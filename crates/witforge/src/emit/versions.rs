//! Version-directory discovery and validation shared by the generators.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::diagnostic::{Diagnostic, LocatedDiagnostic, Position, RelatedLocation, codes};
use crate::lock::{Changes, PackageName, WireLock};
use crate::model::{self, Dependency, Interface, Model, TypeDef, TypeDefKind};
use crate::{ContractError, load, operational_error, operational_error_at, subset};

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
    /// Physical source locations indexed by logical contract path.
    pub(crate) locations: BTreeMap<String, (String, Position)>,
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
        let message = format!(
            "dependency graph differs: {change}; every version in a generated WIT root must use \
             an identical pinned dependency graph"
        );
        bail!(ContractError::with_diagnostic(Diagnostic::error(
            codes::DEPENDENCY_GRAPH_MISMATCH,
            message,
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
#[allow(clippy::too_many_lines)]
pub(crate) fn load_versions(wit_root: &Path) -> Result<Vec<Version>> {
    let released = crate::load_root_released_manifest(wit_root, None)?;
    let mut versions: Vec<Version> = Vec::new();
    for (major, minor, directory) in discover_versions(wit_root)? {
        let contract = load(&directory)?;
        let violations = subset::check_located_diagnostics(&contract);
        if !violations.is_empty() {
            let messages: Vec<String> = violations
                .iter()
                .map(|located| match &located.diagnostic.path {
                    Some(path) => format!("{path}: {}", located.diagnostic.message),
                    None => located.diagnostic.message.clone(),
                })
                .collect();
            bail!(ContractError::with_located_diagnostics(
                format!("{}: {}", directory.display(), messages.join("; ")),
                violations,
            ));
        }
        let locations = contract.member_locations();
        let model = model::build(&contract)?;
        let pkg_key = format!("{}:{}", model.package.namespace, model.package.name);
        let version = model.package.version;
        if version.major != major || version.minor != minor || version.patch != 0 {
            let mut diag = Diagnostic::error(
                codes::DIRECTORY_VERSION_MISMATCH,
                format!(
                    "{} declares version {}, expected {major}.{minor}.0",
                    directory.display(),
                    version
                ),
            )
            .with_path(directory.display().to_string());
            let mut located = LocatedDiagnostic::from_diagnostic(diag.clone());
            if let Some((file, pos)) = locations.get(&pkg_key) {
                diag = diag.with_position(*pos);
                located = LocatedDiagnostic::from_diagnostic(diag)
                    .with_file(file.clone())
                    .with_logical(pkg_key);
            } else {
                located = located.with_file(directory.display().to_string());
            }
            bail!(ContractError::with_located_diagnostic(located));
        }
        if let Some(first) = versions.first() {
            let expected = &first.model.package;
            if expected.namespace != model.package.namespace || expected.name != model.package.name
            {
                let mut diag = Diagnostic::error(
                    codes::PACKAGE_MISMATCH,
                    format!(
                        "{} declares package {}:{}, expected {}:{}",
                        directory.display(),
                        model.package.namespace,
                        model.package.name,
                        expected.namespace,
                        expected.name
                    ),
                )
                .with_path(directory.display().to_string());
                let mut located = LocatedDiagnostic::from_diagnostic(diag.clone());
                if let Some((file, pos)) = locations.get(&pkg_key) {
                    diag = diag.with_position(*pos);
                    located = LocatedDiagnostic::from_diagnostic(diag)
                        .with_file(file.clone())
                        .with_logical(pkg_key);
                } else {
                    located = located.with_file(directory.display().to_string());
                }
                bail!(ContractError::with_located_diagnostic(located));
            }
        }
        let previous = versions.last().filter(|previous| previous.major == major);
        let lock_path = directory.join("wire.lock");
        let lock_text = read_version_lock(&directory, major, &lock_path, previous)?;
        let lock = WireLock::parse(&lock_text).map_err(|err| {
            let position = crate::error_diagnostics(&err)
                .and_then(|diags| diags.first())
                .and_then(|diag| diag.position);
            operational_error_at(
                codes::PARSE_ERROR,
                format!("in {}: {err:#}", lock_path.display()),
                lock_path.display().to_string(),
                position,
            )
        })?;
        let lock_positions = WireLock::entry_positions(&lock_text);
        let package = PackageName {
            namespace: model.package.namespace.clone(),
            name: model.package.name.clone(),
        };
        lock.check_package(&package).map_err(|err| {
            let human = format!("in {}: {err:#}", lock_path.display());
            let lock_file = lock_path.display().to_string();
            let foreign_pos = lock
                .entries()
                .find(|(key, _)| key.package() != &package)
                .and_then(|(key, _)| lock_positions.get(&key.key()).copied());
            let mut diag = Diagnostic::error(codes::LOCKFILE_WRONG_PACKAGE, format!("{err:#}"))
                .with_path(lock_file.clone());
            if let Some(pos) = foreign_pos {
                diag = diag.with_position(pos);
            }
            let located = LocatedDiagnostic::from_diagnostic(diag).with_file(lock_file);
            ContractError::with_located_diagnostics(human, vec![located])
        })?;
        if let Some(previous) = previous {
            let previous_path = previous.directory.join("wire.lock");
            lock.check_append_only(&previous.lock).map_err(|err| {
                let message = format!(
                    "{} does not continue {}: {err:#}; copy {} to {} and run `witforge lock {} \
                     --against {}`",
                    lock_path.display(),
                    previous_path.display(),
                    previous_path.display(),
                    lock_path.display(),
                    crate::shell_quote(&directory),
                    crate::shell_quote(&previous_path),
                );
                let lock_file = lock_path.display().to_string();
                let diag =
                    Diagnostic::error(codes::LOCKFILE_ERROR, message).with_path(lock_file.clone());
                let located = LocatedDiagnostic::from_diagnostic(diag)
                    .with_file(lock_file)
                    .with_related(RelatedLocation {
                        file: previous_path.display().to_string(),
                        position: None,
                        message: "previous minor lockfile".to_string(),
                    });
                ContractError::with_located_diagnostic(located)
            })?;
        }
        let members = model.members()?;
        let is_released = directory
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .is_some_and(|name| released.contains(name));
        let drift = lock.drift(&members).map_err(|err| {
            let human = format!("in {}: {err:#}", lock_path.display());
            let lock_file = lock_path.display().to_string();
            let diag = Diagnostic::error(codes::LOCKFILE_ERROR, format!("{err:#}"))
                .with_path(lock_file.clone());
            let located = LocatedDiagnostic::from_diagnostic(diag).with_file(lock_file);
            ContractError::with_located_diagnostics(human, vec![located])
        })?;
        if !drift.is_empty() {
            let remedy = version_lock_remedy(&directory, previous, is_released);
            let diagnostics = drift_diagnostics(
                &drift,
                &remedy,
                &locations,
                &lock_path.display().to_string(),
                &lock_positions,
            );
            bail!(ContractError::with_located_diagnostics(
                format!("{}: lockfile is stale; {remedy}", directory.display()),
                diagnostics,
            ));
        }
        lock.validate_order_section(&members, is_released)
            .map_err(|err| {
                let message = format!(
                    "{}: {err:#}; {}",
                    lock_path.display(),
                    version_lock_remedy(&directory, previous, is_released)
                );
                let lock_file = lock_path.display().to_string();
                let diag =
                    Diagnostic::error(codes::LOCKFILE_ERROR, message).with_path(lock_file.clone());
                let located = LocatedDiagnostic::from_diagnostic(diag).with_file(lock_file);
                ContractError::with_located_diagnostic(located)
            })?;
        versions.push(Version {
            major,
            directory,
            model,
            lock,
            locations,
        });
    }
    Ok(versions)
}

fn drift_diagnostics(
    changes: &Changes,
    remedy: &str,
    locations: &BTreeMap<String, (String, Position)>,
    lock_file: &str,
    lock_positions: &BTreeMap<String, Position>,
) -> Vec<LocatedDiagnostic> {
    let mut diagnostics = Vec::new();
    let locate_member = |code: &'static str,
                         key: &str,
                         message: String,
                         prefer_lock: bool,
                         related_msg: Option<&str>| {
        let mut diag = Diagnostic::error(code, message).with_path(key.to_string());
        let wit_loc = locations.get(key);
        let lock_pos = lock_positions.get(key).copied();
        let mut located = if prefer_lock {
            if let Some(pos) = lock_pos {
                diag = diag.with_position(pos);
            }
            LocatedDiagnostic::from_diagnostic(diag)
                .with_file(lock_file.to_string())
                .with_logical(key.to_string())
        } else if let Some((file, pos)) = wit_loc {
            diag = diag.with_position(*pos);
            let mut item = LocatedDiagnostic::from_diagnostic(diag)
                .with_file(file.clone())
                .with_logical(key.to_string());
            if let Some(msg) = related_msg {
                item = item.with_related(RelatedLocation {
                    file: lock_file.to_string(),
                    position: lock_pos,
                    message: msg.to_string(),
                });
            }
            item
        } else {
            if let Some(pos) = lock_pos {
                diag = diag.with_position(pos);
            }
            LocatedDiagnostic::from_diagnostic(diag)
                .with_file(lock_file.to_string())
                .with_logical(key.to_string())
        };
        if prefer_lock && let (Some((file, pos)), Some(msg)) = (wit_loc, related_msg) {
            located = located.with_related(RelatedLocation {
                file: file.clone(),
                position: Some(*pos),
                message: msg.to_string(),
            });
        }
        located
    };

    for (key, _) in &changes.added {
        diagnostics.push(locate_member(
            codes::LOCKFILE_MISSING_NUMBER,
            key,
            format!("{key} has no wire number; {remedy}"),
            false,
            Some("wire.lock has no entry for this member"),
        ));
    }
    for (key, number) in &changes.reserved {
        diagnostics.push(locate_member(
            codes::LOCKFILE_UNRESERVED_REMOVAL,
            key,
            format!("{key} = {number} is no longer in the contract; {remedy}"),
            true,
            None,
        ));
    }
    for (key, number) in &changes.reintroduced {
        diagnostics.push(locate_member(
            codes::LOCKFILE_REINTRODUCED,
            key,
            format!(
                "{key} was removed and its name and number {number} stay reserved; give the \
                 member a new name"
            ),
            false,
            Some("reserved entry in wire.lock"),
        ));
    }
    for (key, number) in &changes.dropped {
        diagnostics.push(locate_member(
            codes::LOCKFILE_STALE_DRAFT,
            key,
            format!("stale draft {key} = {number}; {remedy}"),
            true,
            None,
        ));
    }
    for (key, _, _) in &changes.updated {
        diagnostics.push(locate_member(
            codes::LOCKFILE_CHANGED_DRAFT,
            key,
            format!("draft {key} changed kind or type; {remedy}"),
            false,
            Some("existing entry in wire.lock"),
        ));
    }
    diagnostics
}

fn read_version_lock(
    directory: &Path,
    major: u64,
    lock_path: &Path,
    previous: Option<&Version>,
) -> Result<String> {
    match fs::read_to_string(lock_path) {
        Ok(text) => Ok(text),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            let quoted_dir = crate::shell_quote(directory);
            let message = if let Some(previous) = previous {
                let prev_lock = previous.directory.join("wire.lock");
                format!(
                    "lockfile {} does not exist; major {major} already has {}, so copy {} to \
                     {} and run `witforge lock {} --against {}` (or for a new major, create a \
                     fresh lockfile with `--first`)",
                    lock_path.display(),
                    previous.directory.display(),
                    prev_lock.display(),
                    lock_path.display(),
                    quoted_dir,
                    crate::shell_quote(&prev_lock),
                )
            } else {
                format!(
                    "lockfile {} does not exist; for the first version of a package or of a new \
                     major, create it with `witforge lock {} --first`; for a new minor, copy the \
                     previous minor's wire.lock to {} and run witforge lock on that directory with \
                     --against pointing at the previous minor's wire.lock",
                    lock_path.display(),
                    quoted_dir,
                    lock_path.display(),
                )
            };
            Err(operational_error(
                codes::IO_ERROR,
                message,
                Some(lock_path.display().to_string()),
            ))
        }
        Err(err) => Err(err).with_context(|| format!("failed to read {}", lock_path.display())),
    }
}

fn version_lock_remedy(directory: &Path, previous: Option<&Version>, is_released: bool) -> String {
    if is_released {
        format!(
            "`{}` is released, so `witforge lock` refuses it; restore its released WIT and \
             wire.lock",
            directory.display()
        )
    } else if let Some(previous) = previous {
        let previous_path = previous.directory.join("wire.lock");
        format!(
            "run `witforge lock {} --against {}`",
            crate::shell_quote(directory),
            crate::shell_quote(&previous_path)
        )
    } else {
        format!("run `witforge lock {}`", crate::shell_quote(directory))
    }
}

fn discover_versions(wit_root: &Path) -> Result<Vec<(u64, u64, PathBuf)>> {
    crate::discover_version_directories(wit_root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Field, Function, Package, Stability, TypeExpr, Version as ModelVersion};

    fn package(name: &str) -> Package {
        Package {
            namespace: "example".to_string(),
            name: name.to_string(),
            version: ModelVersion {
                major: 0,
                minor: 1,
                patch: 0,
            },
            docs: String::new(),
        }
    }

    fn field(name: &str, ty: TypeExpr) -> Field {
        Field {
            name: name.to_string(),
            docs: String::new(),
            ty,
        }
    }

    fn thing() -> TypeDef {
        TypeDef {
            name: "thing".to_string(),
            docs: String::new(),
            stability: Stability::Unknown,
            kind: TypeDefKind::Record(vec![field("value", TypeExpr::String)]),
        }
    }

    fn extra_type() -> TypeDef {
        TypeDef {
            name: "extra".to_string(),
            docs: String::new(),
            stability: Stability::Unknown,
            kind: TypeDefKind::Record(vec![field("value", TypeExpr::String)]),
        }
    }

    fn ping() -> Function {
        Function {
            name: "ping".to_string(),
            docs: String::new(),
            stability: Stability::Unknown,
            params: Vec::new(),
            result: None,
        }
    }

    fn pong() -> Function {
        Function {
            name: "pong".to_string(),
            docs: String::new(),
            stability: Stability::Unknown,
            params: Vec::new(),
            result: None,
        }
    }

    fn extra_interface() -> Interface {
        Interface {
            name: "other".to_string(),
            docs: String::new(),
            stability: Stability::Unknown,
            types: Vec::new(),
            functions: Vec::new(),
        }
    }

    fn dependency(name: &str) -> Dependency {
        Dependency {
            package: package(name),
            interfaces: vec![Interface {
                name: "types".to_string(),
                docs: String::new(),
                stability: Stability::Unknown,
                types: vec![thing()],
                functions: vec![ping()],
            }],
        }
    }

    fn assert_change(old: &[Dependency], new: &[Dependency], expected: &str) {
        let change = dependency_graph_change(old, new).expect("expected a dependency change");
        assert!(change.contains(expected), "{change}");
    }

    #[allow(clippy::too_many_lines)]
    #[test]
    fn dependency_graph_change_covers_api_and_order_branches() {
        let base = vec![dependency("shared")];

        let mut changed = base.clone();
        changed[0].package.docs = "changed".to_string();
        assert_change(&base, &changed, "package `example:shared` docs change");

        let mut other = dependency("other");
        other.interfaces.clear();
        assert_change(
            &[dependency("shared"), other.clone()],
            &[other, dependency("shared")],
            "dependency package order changes",
        );

        let mut removed = base.clone();
        removed[0].interfaces.clear();
        assert_change(
            &base,
            &removed,
            "interface `example:shared/types` was removed",
        );

        let mut added = base.clone();
        added[0].interfaces.push(extra_interface());
        assert_change(&base, &added, "interface `example:shared/other` was added");

        let mut metadata = base.clone();
        metadata[0].interfaces[0].docs = "changed".to_string();
        assert_change(
            &base,
            &metadata,
            "interface `example:shared/types` metadata changes",
        );

        let mut interface_order = base.clone();
        interface_order[0].interfaces.push(extra_interface());
        let mut reversed = interface_order.clone();
        reversed[0].interfaces.reverse();
        assert_change(
            &interface_order,
            &reversed,
            "interface order in `example:shared` changes",
        );

        let mut type_removed = base.clone();
        type_removed[0].interfaces[0].types.push(extra_type());
        assert_change(
            &type_removed,
            &base,
            "type `example:shared/types/extra` was removed",
        );
        assert_change(
            &base,
            &type_removed,
            "type `example:shared/types/extra` was added",
        );

        let mut type_order = type_removed.clone();
        type_order[0].interfaces[0].types.reverse();
        assert_change(
            &type_removed,
            &type_order,
            "type order in `example:shared/types` changes",
        );

        let mut shape = base.clone();
        shape[0].interfaces[0].types[0].kind =
            TypeDefKind::Record(vec![field("value", TypeExpr::U64)]);
        assert_change(
            &base,
            &shape,
            "type `example:shared/types/thing` shape changes",
        );

        let mut member_metadata = base.clone();
        if let TypeDefKind::Record(fields) = &mut member_metadata[0].interfaces[0].types[0].kind {
            fields[0].docs = "changed".to_string();
        }
        assert_change(
            &base,
            &member_metadata,
            "type `example:shared/types/thing` member metadata changes",
        );

        let mut type_metadata = base.clone();
        type_metadata[0].interfaces[0].types[0].docs = "changed".to_string();
        assert_change(
            &base,
            &type_metadata,
            "type `example:shared/types/thing` metadata changes",
        );

        let mut function_removed = base.clone();
        function_removed[0].interfaces[0].functions.push(pong());
        assert_change(
            &function_removed,
            &base,
            "function `example:shared/types#pong` was removed",
        );
        assert_change(
            &base,
            &function_removed,
            "function `example:shared/types#pong` was added",
        );

        let mut function_order = function_removed.clone();
        function_order[0].interfaces[0].functions.reverse();
        assert_change(
            &function_removed,
            &function_order,
            "function order in `example:shared/types` changes",
        );

        let mut signature = base.clone();
        signature[0].interfaces[0].functions[0].result = Some(TypeExpr::U64);
        assert_change(
            &base,
            &signature,
            "function `example:shared/types#ping` signature changes",
        );

        let mut function_metadata = base.clone();
        function_metadata[0].interfaces[0].functions[0].docs = "changed".to_string();
        assert_change(
            &base,
            &function_metadata,
            "function `example:shared/types#ping` metadata changes",
        );
    }
}
