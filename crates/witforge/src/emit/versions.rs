//! Version-directory discovery and validation shared by the generators.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::lock::{PackageName, WireLock};
use crate::model::{self, Dependency, Interface, Model, TypeDef, TypeDefKind};
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

/// Return the one dependency graph shared by all generated versions.
///
/// Dependency bindings are emitted once and used by every generated version. Exact graph
/// equality therefore includes package pins, source order, API shapes, stability, and docs.
pub(crate) fn shared_dependencies(versions: &[Version]) -> Result<&[Dependency]> {
    let first = versions
        .first()
        .context("no version directories found under WIT root")?;
    for version in &versions[1..] {
        ensure_same_dependency_graph(&first.model.dependencies, &version.model.dependencies)?;
    }
    Ok(&first.model.dependencies)
}

/// Refuse two dependency graphs that cannot share one generated binding surface.
pub(crate) fn ensure_same_dependency_graph(old: &[Dependency], new: &[Dependency]) -> Result<()> {
    if let Some(change) = dependency_graph_change(old, new) {
        bail!(ContractError::new(format!(
            "dependency graph differs: {change}; every version in a generated WIT root must use \
             an identical pinned dependency graph"
        )));
    }
    Ok(())
}

fn dependency_graph_change(old: &[Dependency], new: &[Dependency]) -> Option<String> {
    for dependency in old {
        let name = dependency_name(dependency);
        let Some(candidate) = find_dependency(new, dependency) else {
            return Some(format!("dependency package `{name}` was removed"));
        };
        if dependency.package.version != candidate.package.version {
            return Some(format!(
                "dependency package `{name}` version changes from {} to {}",
                dependency.package.version, candidate.package.version
            ));
        }
        if dependency.package.docs != candidate.package.docs {
            return Some(format!("dependency package `{name}` docs change"));
        }
        if let Some(change) =
            dependency_interface_change(&name, &dependency.interfaces, &candidate.interfaces)
        {
            return Some(change);
        }
    }
    for dependency in new {
        if find_dependency(old, dependency).is_none() {
            return Some(format!(
                "dependency package `{}` was added",
                dependency_name(dependency)
            ));
        }
    }
    if !old.iter().zip(new).all(|(old, new)| {
        old.package.namespace == new.package.namespace && old.package.name == new.package.name
    }) {
        return Some("dependency package order changes".to_string());
    }
    None
}

fn dependency_interface_change(
    package: &str,
    old: &[Interface],
    new: &[Interface],
) -> Option<String> {
    for interface in old {
        let qualified = format!("{package}/{}", interface.name);
        let Some(candidate) = new.iter().find(|item| item.name == interface.name) else {
            return Some(format!("dependency interface `{qualified}` was removed"));
        };
        if interface.docs != candidate.docs || interface.stability != candidate.stability {
            return Some(format!(
                "dependency interface `{qualified}` metadata changes"
            ));
        }
        for ty in &interface.types {
            let path = format!("{qualified}/{}", ty.name);
            let Some(candidate_ty) = candidate.types.iter().find(|item| item.name == ty.name)
            else {
                return Some(format!("dependency type `{path}` was removed"));
            };
            if !same_type_shape(&ty.kind, &candidate_ty.kind) {
                return Some(format!("dependency type `{path}` shape changes"));
            }
            if ty.kind != candidate_ty.kind {
                return Some(format!("dependency type `{path}` member metadata changes"));
            }
            if ty.docs != candidate_ty.docs || ty.stability != candidate_ty.stability {
                return Some(format!("dependency type `{path}` metadata changes"));
            }
        }
        for ty in &candidate.types {
            if interface.types.iter().all(|item| item.name != ty.name) {
                return Some(format!(
                    "dependency type `{qualified}/{}` was added",
                    ty.name
                ));
            }
        }
        if !same_type_order(&interface.types, &candidate.types) {
            return Some(format!("dependency type order in `{qualified}` changes"));
        }
        for function in &interface.functions {
            let path = format!("{qualified}#{}", function.name);
            let Some(candidate_function) = candidate
                .functions
                .iter()
                .find(|item| item.name == function.name)
            else {
                return Some(format!("dependency function `{path}` was removed"));
            };
            if function.params != candidate_function.params
                || function.result != candidate_function.result
            {
                return Some(format!("dependency function `{path}` signature changes"));
            }
            if function.docs != candidate_function.docs
                || function.stability != candidate_function.stability
            {
                return Some(format!("dependency function `{path}` metadata changes"));
            }
        }
        for function in &candidate.functions {
            if interface
                .functions
                .iter()
                .all(|item| item.name != function.name)
            {
                return Some(format!(
                    "dependency function `{qualified}#{}` was added",
                    function.name
                ));
            }
        }
        if !interface
            .functions
            .iter()
            .map(|item| &item.name)
            .eq(candidate.functions.iter().map(|item| &item.name))
        {
            return Some(format!(
                "dependency function order in `{qualified}` changes"
            ));
        }
    }
    for interface in new {
        if old.iter().all(|item| item.name != interface.name) {
            return Some(format!(
                "dependency interface `{package}/{}` was added",
                interface.name
            ));
        }
    }
    if !old
        .iter()
        .map(|item| &item.name)
        .eq(new.iter().map(|item| &item.name))
    {
        return Some(format!("dependency interface order in `{package}` changes"));
    }
    None
}

fn same_type_order(old: &[TypeDef], new: &[TypeDef]) -> bool {
    old.iter()
        .map(|item| &item.name)
        .eq(new.iter().map(|item| &item.name))
}

fn same_type_shape(old: &TypeDefKind, new: &TypeDefKind) -> bool {
    match (old, new) {
        (TypeDefKind::Record(old), TypeDefKind::Record(new)) => {
            old.len() == new.len()
                && old
                    .iter()
                    .zip(new)
                    .all(|(old, new)| old.name == new.name && old.ty == new.ty)
        }
        (TypeDefKind::Variant(old), TypeDefKind::Variant(new))
        | (TypeDefKind::Enum(old), TypeDefKind::Enum(new)) => {
            old.len() == new.len()
                && old
                    .iter()
                    .zip(new)
                    .all(|(old, new)| old.name == new.name && old.payload == new.payload)
        }
        (TypeDefKind::Flags(old), TypeDefKind::Flags(new)) => {
            old.len() == new.len() && old.iter().zip(new).all(|(old, new)| old.name == new.name)
        }
        (TypeDefKind::Alias(old), TypeDefKind::Alias(new))
        | (TypeDefKind::List(old), TypeDefKind::List(new))
        | (TypeDefKind::Option(old), TypeDefKind::Option(new)) => old == new,
        (TypeDefKind::Result(old), TypeDefKind::Result(new)) => old == new,
        _ => false,
    }
}

fn find_dependency<'a>(
    dependencies: &'a [Dependency],
    target: &Dependency,
) -> Option<&'a Dependency> {
    dependencies.iter().find(|dependency| {
        dependency.package.namespace == target.package.namespace
            && dependency.package.name == target.package.name
    })
}

fn dependency_name(dependency: &Dependency) -> String {
    format!(
        "{}:{}",
        dependency.package.namespace, dependency.package.name
    )
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
/// Returns an error for a missing, duplicate, or symlinked version directory, an unparsable WIT
/// package, or an unreadable or invalid lockfile, and a [`ContractError`] for a subset
/// violation, a version or package mismatch, a lockfile entry of another package, a lockfile
/// that does not continue the previous minor's, or a stale lockfile.
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
        let package = PackageName {
            namespace: model.package.namespace.clone(),
            name: model.package.name.clone(),
        };
        lock.check_package(&package).map_err(|err| {
            ContractError::from_error(&err.context(format!("in {}", lock_path.display())))
        })?;
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
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some((major, minor)) = crate::parse_version_directory(&name)? else {
            continue;
        };
        let path = entry.path();
        // A symlinked version directory is refused rather than followed or skipped: following
        // it would let a released version change through the link's target, and skipping it
        // would silently leave that version out of the generated output.
        let file_type = entry
            .file_type()
            .with_context(|| format!("failed to read version directory {}", path.display()))?;
        if file_type.is_symlink() {
            crate::ensure_version_directory(&path)?;
        }
        if !file_type.is_dir() {
            continue;
        }
        versions.push((major, minor, path));
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
