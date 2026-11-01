//! witforge: a WIT-first contract compiler.
//!
//! A WIT package is the single source of truth for a contract. witforge checks that the package
//! stays inside a restricted, pure subset of WIT that maps cleanly onto every target, keeps a
//! committed, append-only lockfile of wire numbers so the protobuf projection stays stable across
//! versions, and emits plain Rust types and traits, a `.proto` schema, and the glue that converts
//! between the two.
//!
//! - [`load`] parses a version directory's WIT package, and [`subset`] checks it against the
//!   supported subset.
//! - [`model`] is the owned contract model the lockfile and the emitters consume.
//! - [`lock`] reads, reconciles, and writes the `wire.lock` lockfile, and
//!   [`ReleasedManifest`] reads the `released` manifest of frozen versions.
//! - [`emit`] generates the [Rust](emit::rust), [protobuf](emit::proto), and
//!   [glue](emit::glue) output for a WIT root and checks committed output for drift.
//! - [`ContractError`] marks contract, lockfile, and output violations, as distinct from usage,
//!   I/O, and parse errors.
//!
//! The `witforge` command-line interface (the `witforge-cli` crate) wraps these as the `check`,
//! `lock`, `rust`, `proto`, and `glue` commands. The normative specifications are in
//! [`docs/design`](https://github.com/asamarts/witforge/tree/main/docs/design).

pub mod emit;
pub mod lock;
pub mod model;
pub mod subset;

use std::collections::BTreeSet;
use std::fmt;
use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail};
use wit_parser::{PackageId, Resolve};

/// A contract, lockfile, or generated-output violation, as opposed to a usage, I/O, parse, or
/// internal error.
///
/// The generators ([`emit::rust`], [`emit::proto`], and [`emit::glue`]) report a contract or
/// lockfile violation as this type inside the returned [`anyhow::Error`], so callers can tell
/// the two classes apart with [`is_contract_error`]; the `witforge` CLI does the same for `check`
/// and `lock`, and exits with `1` for a violation and `2` for any other error. The lower-level
/// [`lock::WireLock`] methods return plain errors, which their callers classify.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractError {
    message: String,
}

impl ContractError {
    /// A violation described by `message`.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// Classify `error` as a violation, keeping its whole context chain in the message.
    #[must_use]
    pub fn from_error(error: &anyhow::Error) -> Self {
        Self::new(format!("{error:#}"))
    }
}

impl fmt::Display for ContractError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ContractError {}

/// Whether `error`, or any error in its context chain, is a [`ContractError`].
#[must_use]
pub fn is_contract_error(error: &anyhow::Error) -> bool {
    error
        .chain()
        .any(<dyn std::error::Error>::is::<ContractError>)
}

/// A parsed WIT package together with the resolve that owns it.
///
/// The fields are crate-private so that `wit-parser` types do not become part of witforge's
/// public API. Callers use [`load`] and the model/subset/lock modules instead.
///
/// The boundary is compile-time enforced; external code cannot reach the resolved package:
///
/// ```compile_fail
/// let contract = witforge::load(".").unwrap();
/// let _ = contract.resolve;
/// ```
#[derive(Debug)]
pub struct Contract {
    pub(crate) resolve: Resolve,
    pub(crate) package: PackageId,
}

/// Load the WIT package in `dir`.
///
/// All features are enabled, so items gated by `@unstable` are kept; otherwise they would
/// silently disappear and the lockfile would report their members as removed.
///
/// # Errors
///
/// Returns an error when the directory cannot be read or the WIT does not parse or resolve.
pub fn load(dir: &Path) -> Result<Contract> {
    let mut resolve = Resolve {
        all_features: true,
        ..Resolve::default()
    };
    let (package, _sources) = resolve
        .push_dir(dir)
        .with_context(|| format!("failed to load the WIT package in {}", dir.display()))?;
    Ok(Contract { resolve, package })
}

/// The default released-version manifest name.
pub const RELEASED_MANIFEST: &str = "released";

/// The validated contents of a `wit/released` manifest.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReleasedManifest {
    directories: Vec<String>,
}

impl ReleasedManifest {
    /// Parse and validate a released manifest.
    ///
    /// Entries are plain directory names relative to the manifest's directory: an ASCII letter
    /// or digit followed by ASCII letters, digits, `.`, `_`, or `-`
    /// (`^[A-Za-z0-9][A-Za-z0-9._-]*$`). That excludes `.`, `..`, hidden names, whitespace,
    /// separators, and absolute paths. Leading and trailing ASCII whitespace (space, tab, CR,
    /// VT, FF) is trimmed; blank lines and `#` comments are ignored. Invalid, duplicate,
    /// missing, and lockfile-less directories are errors.
    ///
    /// # Errors
    ///
    /// Returns an error for an unreadable manifest or any invalid entry.
    pub fn load(path: &Path) -> Result<Self> {
        let text = fs::read_to_string(path)
            .with_context(|| format!("failed to read released manifest {}", path.display()))?;
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        let mut directories = Vec::new();
        let mut seen = BTreeSet::new();
        for (index, raw) in text.lines().enumerate() {
            let line_no = index + 1;
            let line = raw.trim_matches([' ', '\t', '\r', '\x0b', '\x0c']);
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if !plain_directory_name(line) {
                bail!(
                    "line {line_no}: released entry `{line}` is not a plain directory name \
                     (`^[A-Za-z0-9][A-Za-z0-9._-]*$`)"
                );
            }
            if !seen.insert(line.to_string()) {
                bail!("line {line_no}: duplicate released entry `{line}`");
            }
            let directory = parent.join(line);
            if !directory.is_dir() {
                bail!("line {line_no}: released directory `{line}` does not exist");
            }
            if !directory.join("wire.lock").is_file() {
                bail!("line {line_no}: released directory `{line}` has no wire.lock");
            }
            directories.push(line.to_string());
        }
        Ok(Self { directories })
    }

    /// The released directory names in manifest order.
    #[must_use]
    pub fn directories(&self) -> &[String] {
        &self.directories
    }

    /// Whether `directory` is listed.
    #[must_use]
    pub fn contains(&self, directory: &str) -> bool {
        self.directories.iter().any(|entry| entry == directory)
    }
}

/// Whether `name` matches `^[A-Za-z0-9][A-Za-z0-9._-]*$`, the released-manifest entry rule shared
/// with `ci/check-release-freeze.sh`.
fn plain_directory_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    bytes
        .next()
        .is_some_and(|first| first.is_ascii_alphanumeric())
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

/// The default manifest path for a version directory: `<wit-dir>/../released`.
///
/// The version directory is canonicalised first, so `.` and a symlinked directory name the
/// directory's real parent. A directory that cannot be resolved is used as given.
#[must_use]
pub fn default_released_manifest(wit_dir: &Path) -> PathBuf {
    let resolved = fs::canonicalize(wit_dir).unwrap_or_else(|_| wit_dir.to_path_buf());
    resolved
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .join(RELEASED_MANIFEST)
}

/// Load the manifest for a version directory.
///
/// A missing default manifest means no versions are released. An explicitly supplied missing
/// manifest is an error.
///
/// # Errors
///
/// Returns an error for an invalid manifest or an explicitly missing manifest.
pub fn load_released_manifest(wit_dir: &Path, explicit: Option<&Path>) -> Result<ReleasedManifest> {
    load_manifest_or_default(default_released_manifest(wit_dir), explicit)
}

/// Load the manifest for a WIT root, as the generators do; the default is `<wit-root>/released`.
///
/// A missing default manifest means no versions are released. An explicitly supplied missing
/// manifest is an error.
///
/// # Errors
///
/// Returns an error for an invalid manifest or an explicitly missing manifest.
pub fn load_root_released_manifest(
    wit_root: &Path,
    explicit: Option<&Path>,
) -> Result<ReleasedManifest> {
    load_manifest_or_default(wit_root.join(RELEASED_MANIFEST), explicit)
}

fn load_manifest_or_default(default: PathBuf, explicit: Option<&Path>) -> Result<ReleasedManifest> {
    let path = explicit.map_or(default, Path::to_path_buf);
    if explicit.is_none() && !path.exists() {
        return Ok(ReleasedManifest::default());
    }
    ReleasedManifest::load(&path)
}

/// Refuse to lock a version directory listed in `wit/released`.
///
/// The decision compares resolved paths, not names: `wit_dir` and every listed directory are
/// canonicalised, so `.`, `..` components, and symlinks cannot hide a released directory. With no
/// explicit manifest, the `released` manifest beside the directory is read both where the
/// directory is written (its lexical parent) and where it resolves to (its canonical parent), so
/// a version directory that is a symlink is refused by the manifest of the WIT root that lists it.
///
/// # Errors
///
/// Returns an error when the directory cannot be resolved, is released, or a manifest is
/// invalid.
pub fn ensure_not_released(wit_dir: &Path, explicit: Option<&Path>) -> Result<()> {
    let directory = fs::canonicalize(wit_dir)
        .with_context(|| format!("failed to resolve version directory {}", wit_dir.display()))?;
    for released in released_directories(wit_dir, explicit)? {
        if released.directory == directory {
            bail!(
                "version directory `{}` is released (listed as `{}` in {}) and cannot be locked",
                wit_dir.display(),
                released.entry,
                released.manifest.display()
            );
        }
    }
    Ok(())
}

/// Refuse to write a lockfile into a released version directory, as [`ensure_not_released`]
/// refuses to lock one: `lock --lock <path>` must not rewrite a released `wire.lock`.
///
/// The lockfile's directory is canonicalised and compared with every directory the manifests
/// that apply to `wit_dir` release (see [`ensure_not_released`]).
///
/// # Errors
///
/// Returns an error when the lockfile's directory cannot be resolved, is or is inside a
/// released directory, or a manifest is invalid.
pub fn ensure_lockfile_not_released(
    lock: &Path,
    wit_dir: &Path,
    explicit: Option<&Path>,
) -> Result<()> {
    let parent = lock
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let parent = fs::canonicalize(parent).with_context(|| {
        format!(
            "failed to resolve the directory of lockfile {}",
            lock.display()
        )
    })?;
    for released in released_directories(wit_dir, explicit)? {
        if parent.starts_with(&released.directory) {
            bail!(
                "lockfile `{}` is in the released directory `{}` (listed as `{}` in {}) and \
                 cannot be written",
                lock.display(),
                released.directory.display(),
                released.entry,
                released.manifest.display()
            );
        }
    }
    Ok(())
}

/// A directory a released manifest lists, resolved.
struct ReleasedDirectory {
    /// The canonical directory.
    directory: PathBuf,
    /// The manifest entry that names it.
    entry: String,
    /// The manifest that lists it.
    manifest: PathBuf,
}

/// Every directory released by the manifests that apply to `wit_dir`: the explicit manifest
/// alone, or the default manifest beside the directory's lexical and canonical locations.
fn released_directories(wit_dir: &Path, explicit: Option<&Path>) -> Result<Vec<ReleasedDirectory>> {
    let mut manifests = Vec::new();
    if let Some(path) = explicit {
        manifests.push(path.to_path_buf());
    } else {
        manifests.push(default_released_manifest(wit_dir));
        let lexical = lexical_absolute(wit_dir)?;
        if let Some(parent) = lexical.parent() {
            let candidate = parent.join(RELEASED_MANIFEST);
            if !manifests.contains(&candidate) {
                manifests.push(candidate);
            }
        }
        // A missing default manifest releases nothing.
        manifests.retain(|path| path.exists());
    }
    let mut released = Vec::new();
    for path in manifests {
        let manifest = ReleasedManifest::load(&path)?;
        let base = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        for entry in manifest.directories() {
            let listed = base.join(entry);
            let directory = fs::canonicalize(&listed).with_context(|| {
                format!("failed to resolve released directory {}", listed.display())
            })?;
            released.push(ReleasedDirectory {
                directory,
                entry: entry.clone(),
                manifest: path.clone(),
            });
        }
    }
    Ok(released)
}

/// `path` made absolute with its `.` and `..` components removed lexically, as written rather
/// than as the file system resolves it.
fn lexical_absolute(path: &Path) -> Result<PathBuf> {
    let absolute = std::path::absolute(path)
        .with_context(|| format!("failed to resolve {}", path.display()))?;
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other),
        }
    }
    Ok(normalized)
}
