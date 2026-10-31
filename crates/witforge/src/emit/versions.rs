//! Version-directory discovery and validation shared by the generators.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::lock::WireLock;
use crate::model::{self, Model};
use crate::{ContractError, load, subset};

/// One validated `v<major>.<minor>` version directory.
pub(crate) struct Version {
    /// The major version from the directory name.
    pub(crate) major: u64,
    /// The version directory.
    pub(crate) directory: PathBuf,
    /// The version's model.
    pub(crate) model: Model,
    /// The version's committed lockfile.
    pub(crate) lock: WireLock,
}

/// Discover and validate every version directory under `wit_root`, in ascending semver order.
///
/// Every version is checked against the subset, its directory's version, the package shared by
/// all versions, and its committed lockfile before anything is generated. Within a major, each
/// minor's lockfile must also continue the previous minor's: every previous entry survives with
/// its number, kind, and type, and stays reserved once reserved, as `lock --against` requires.
/// The result is never empty.
///
/// # Errors
///
/// Returns an error for a missing or duplicate version directory, an unparsable WIT package, or
/// an unreadable or invalid lockfile, and a [`ContractError`] for a subset violation, a version or
/// package mismatch, a lockfile that does not continue the previous minor's, or a stale
/// lockfile.
pub(crate) fn load_versions(wit_root: &Path) -> Result<Vec<Version>> {
    let mut versions: Vec<Version> = Vec::new();
    for (major, minor, directory) in discover_versions(wit_root)? {
        let contract = load(&directory)?;
        let violations = subset::check(&contract);
        if !violations.is_empty() {
            let messages: Vec<String> = violations.iter().map(ToString::to_string).collect();
            bail!(ContractError::new(format!(
                "{}: {}",
                directory.display(),
                messages.join("; ")
            )));
        }
        let model = model::build(&contract)?;
        let version = model.package.version;
        if version.major != major || version.minor != minor || version.patch != 0 {
            bail!(ContractError::new(format!(
                "{} declares version {}, expected {major}.{minor}.0",
                directory.display(),
                version
            )));
        }
        if let Some(first) = versions.first() {
            let expected = &first.model.package;
            if expected.namespace != model.package.namespace || expected.name != model.package.name
            {
                bail!(ContractError::new(format!(
                    "{} declares package {}:{}, expected {}:{}",
                    directory.display(),
                    model.package.namespace,
                    model.package.name,
                    expected.namespace,
                    expected.name
                )));
            }
        }
        let lock_path = directory.join("wire.lock");
        let lock_text = fs::read_to_string(&lock_path)
            .with_context(|| format!("failed to read {}", lock_path.display()))?;
        let lock =
            WireLock::parse(&lock_text).with_context(|| format!("in {}", lock_path.display()))?;
        if let Some(previous) = versions.last().filter(|previous| previous.major == major) {
            let previous_path = previous.directory.join("wire.lock");
            lock.check_append_only(&previous.lock).map_err(|err| {
                ContractError::new(format!(
                    "{} does not continue {}: {err:#}; a new minor starts as a copy of the \
                     previous minor's wire.lock and is developed with `witforge lock --against`",
                    lock_path.display(),
                    previous_path.display()
                ))
            })?;
        }
        let members = model.members()?;
        let drift = lock.drift(&members).map_err(|err| {
            ContractError::from_error(&err.context(format!("in {}", lock_path.display())))
        })?;
        if !drift.is_empty() {
            bail!(ContractError::new(format!(
                "{}: lockfile is stale; run `witforge lock`",
                directory.display()
            )));
        }
        versions.push(Version {
            major,
            directory,
            model,
            lock,
        });
    }
    Ok(versions)
}

fn discover_versions(wit_root: &Path) -> Result<Vec<(u64, u64, PathBuf)>> {
    let mut versions = Vec::new();
    for entry in fs::read_dir(wit_root)
        .with_context(|| format!("failed to read WIT root {}", wit_root.display()))?
    {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some((major, minor)) = parse_version_directory(&name) else {
            continue;
        };
        versions.push((major, minor, entry.path()));
    }
    versions.sort_by_key(|(major, minor, _)| (*major, *minor));
    for pair in versions.windows(2) {
        if pair[0].0 == pair[1].0 && pair[0].1 == pair[1].1 {
            bail!(
                "duplicate version directories {} and {}",
                pair[0].2.display(),
                pair[1].2.display()
            );
        }
    }
    if versions.is_empty() {
        bail!(
            "no v<major>.<minor> version directories under {}",
            wit_root.display()
        );
    }
    Ok(versions)
}

fn parse_version_directory(name: &str) -> Option<(u64, u64)> {
    let rest = name.strip_prefix('v')?;
    let (major, minor) = rest.split_once('.')?;
    Some((major.parse().ok()?, minor.parse().ok()?))
}
