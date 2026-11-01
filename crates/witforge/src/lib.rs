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
    /// missing, symlinked, and lockfile-less directories are errors.
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
            if fs::symlink_metadata(&directory)
                .is_ok_and(|metadata| metadata.file_type().is_symlink())
            {
                bail!(
                    "line {line_no}: released directory `{line}` is a symlink; witforge refuses \
                     symlinked version directories"
                );
            }
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
/// The version directory is canonicalised first, so `.` and a symlinked WIT root name the
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

/// Refuse a version directory that is a symlink or not a directory.
///
/// witforge never follows a symlinked version directory: a released version reached through a
/// link could change through the link's target, which neither the released manifest beside the
/// link nor `ci/check-release-freeze.sh` would see. The WIT root and its parents may be symlinks.
/// Trailing separators and `.` components are ignored, so `v0.1/` is checked as `v0.1`.
///
/// # Errors
///
/// Returns an error when the directory is a symlink, is missing, or is not a directory.
pub fn ensure_version_directory(wit_dir: &Path) -> Result<()> {
    let path: PathBuf = wit_dir.components().collect();
    let metadata = fs::symlink_metadata(&path)
        .with_context(|| format!("failed to read version directory {}", wit_dir.display()))?;
    if metadata.file_type().is_symlink() {
        bail!(
            "version directory `{}` is a symlink; witforge refuses symlinked version \
             directories, so a released version cannot change through its link's target",
            wit_dir.display()
        );
    }
    if !metadata.is_dir() {
        bail!(
            "version directory `{}` is not a directory",
            wit_dir.display()
        );
    }
    Ok(())
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

/// Refuse to lock a version directory listed in `wit/released`, or one that is a symlink.
///
/// The decision compares resolved paths, not names: `wit_dir` and every listed directory are
/// canonicalised, so `.` and `..` components and a symlinked WIT root cannot hide a released
/// directory. The `released` manifest beside the directory always applies, read both where the
/// directory is written (its lexical parent) and where it resolves to (its canonical parent); an
/// `explicit` manifest is read as well, never instead.
///
/// # Errors
///
/// Returns an error when the directory is a symlink, cannot be resolved, or is released, or a
/// manifest is invalid.
pub fn ensure_not_released(wit_dir: &Path, explicit: Option<&Path>) -> Result<()> {
    ensure_version_directory(wit_dir)?;
    let directory = fs::canonicalize(wit_dir)
        .with_context(|| format!("failed to resolve version directory {}", wit_dir.display()))?;
    if let Some(released) = released_directories(&[wit_dir], explicit)?
        .into_iter()
        .find(|released| released.directory == directory)
    {
        bail!(
            "version directory `{}` is released (listed as `{}` in {}) and cannot be locked",
            wit_dir.display(),
            released.entry,
            released.manifest.display()
        );
    }
    Ok(())
}

/// Whether `wit_dir` is listed in the manifests [`ensure_not_released`] reads.
///
/// # Errors
///
/// Returns an error when the directory cannot be resolved or a manifest is invalid.
pub fn is_released(wit_dir: &Path, explicit: Option<&Path>) -> Result<bool> {
    let directory = fs::canonicalize(wit_dir)
        .with_context(|| format!("failed to resolve version directory {}", wit_dir.display()))?;
    Ok(released_directories(&[wit_dir], explicit)?
        .iter()
        .any(|released| released.directory == directory))
}

/// Refuse to write a lockfile into a released version directory, as [`ensure_not_released`]
/// refuses to lock one: `lock --lock <path>` must not rewrite a released `wire.lock`.
///
/// The lockfile's directory is canonicalised and compared with every directory released by the
/// manifests [`ensure_not_released`] reads for `wit_dir` and by the `released` manifest beside
/// the lockfile's own directory, so neither a lockfile in another WIT root nor an `explicit`
/// manifest that omits the directory gets past the manifest that lists it.
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
    let resolved = fs::canonicalize(parent).with_context(|| {
        format!(
            "failed to resolve the directory of lockfile {}",
            lock.display()
        )
    })?;
    if let Some(released) = released_directories(&[wit_dir, parent], explicit)?
        .into_iter()
        .find(|released| resolved.starts_with(&released.directory))
    {
        bail!(
            "lockfile `{}` is in the released directory `{}` (listed as `{}` in {}) and \
             cannot be written",
            lock.display(),
            released.directory.display(),
            released.entry,
            released.manifest.display()
        );
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

/// Every directory released by the `explicit` manifest and by the default manifest beside each
/// of `directories`, at its lexical and its canonical location. A missing default manifest
/// releases nothing.
fn released_directories(
    directories: &[&Path],
    explicit: Option<&Path>,
) -> Result<Vec<ReleasedDirectory>> {
    let mut manifests: Vec<PathBuf> = explicit.iter().map(|path| path.to_path_buf()).collect();
    for directory in directories {
        let mut defaults = vec![default_released_manifest(directory)];
        if let Some(parent) = lexical_absolute(directory)?.parent() {
            defaults.push(parent.join(RELEASED_MANIFEST));
        }
        for candidate in defaults {
            if candidate.exists() && !manifests.contains(&candidate) {
                manifests.push(candidate);
            }
        }
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
