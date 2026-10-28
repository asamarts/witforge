//! witforge: a WIT-first contract compiler.
//!
//! A WIT package is the single source of truth for a contract. witforge checks that the package
//! stays inside a restricted, pure subset of WIT that maps cleanly onto every target, keeps a
//! committed, append-only lockfile of wire numbers so the protobuf projection stays stable across
//! versions, and (in later milestones) emits plain Rust types and traits and a `.proto` schema.
//!
//! This first slice provides [`load`], the [`subset`] checker, the owned [`model`], and the
//! [`lock`] lockfile.

pub mod lock;
pub mod model;
pub mod subset;

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use wit_parser::{PackageId, Resolve};

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
    /// Entries are simple directory names relative to the manifest's directory. Blank lines
    /// and `#` comments are ignored. Duplicate, absolute, parent-traversing, missing, and
    /// lockfile-less directories are errors.
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
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let entry = Path::new(line);
            if entry.is_absolute()
                || line == "."
                || line == ".."
                || entry.components().count() != 1
                || line.contains('/')
                || line.contains('\\')
            {
                bail!("line {line_no}: released entry `{line}` must be a simple directory name");
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

/// The default manifest path for a version directory: `<wit-dir>/../released`.
#[must_use]
pub fn default_released_manifest(wit_dir: &Path) -> PathBuf {
    wit_dir
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
    let path = explicit.map_or_else(|| default_released_manifest(wit_dir), Path::to_path_buf);
    if explicit.is_none() && !path.exists() {
        return Ok(ReleasedManifest::default());
    }
    ReleasedManifest::load(&path)
}

/// Refuse to lock a version directory listed in `wit/released`.
///
/// # Errors
///
/// Returns an error when the directory is released or the manifest is invalid.
pub fn ensure_not_released(wit_dir: &Path, explicit: Option<&Path>) -> Result<()> {
    let manifest = load_released_manifest(wit_dir, explicit)?;
    let directory = wit_dir
        .file_name()
        .and_then(|name| name.to_str())
        .with_context(|| format!("version directory `{}` has no name", wit_dir.display()))?;
    if manifest.contains(directory) {
        bail!("version directory `{directory}` is released and cannot be locked");
    }
    Ok(())
}
