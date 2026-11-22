//! Version-directory discovery and validation shared by the generators.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::diagnostic::{Diagnostic, LocatedDiagnostic, Position, RelatedLocation, codes};
use crate::emit::ident;
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

/// Resolved dependency layout across all live majors of a WIT root.
///
/// Within each major, every version must pin an identical dependency graph. Across majors, a
/// dependency package whose definition is identical in every live major that uses it (and whose
/// transitive dependencies are also shared) stays at the single shared location (`witforge_deps`,
/// `<ns>.<pkg>.v<dep_major>.proto`, `<ns>.<pkg>.v<dep_major>.schema.json`). A dependency package
/// that changed or was upgraded across live majors is scoped per root major
/// (`witforge_deps::v<root_major>::<ns>_<pkg>`), and its protobuf / JSON Schema files are
/// disambiguated only when two live majors also share the same dependency major number.
#[derive(Debug, Clone, Default)]
pub(crate) struct DepLayout {
    /// Shared dependencies keyed by `(namespace, name)`.
    pub(crate) shared: BTreeMap<(String, String), Dependency>,
    /// Per-major scoped dependencies keyed by root `major`.
    pub(crate) per_major: BTreeMap<u64, Vec<Dependency>>,
    /// All validated dependencies per root `major` (both shared and scoped).
    pub(crate) by_major: BTreeMap<u64, Vec<Dependency>>,
    /// `(root_major, ns, name)` entries whose `(ns, name, dep_major)` collides with a different
    /// definition in another live root major and therefore uses a root-major-disambiguated proto
    /// package / schema filename.
    pub(crate) disambiguated: BTreeSet<(u64, String, String)>,
    /// For every scoped `(root_major, ns, name)`, the smallest root major that defines the exact
    /// same effective dependency (including transitive dependencies). When `owner < root_major`,
    /// Rust and glue re-export `super::v<owner>::<pkg>` so adjacent majors with identical pinned
    /// dependencies share one Rust type even when a third major upgrades the package.
    pub(crate) canonical_owner: BTreeMap<(u64, String, String), u64>,
}

impl DepLayout {
    /// Validate intra-major dependency identity across `versions` and compute the shared vs
    /// per-major dependency layout.
    #[allow(clippy::too_many_lines)]
    pub(crate) fn from_versions(versions: &[Version]) -> Result<Self> {
        let mut by_major_refs: BTreeMap<u64, Vec<&Version>> = BTreeMap::new();
        for version in versions {
            by_major_refs
                .entry(version.major)
                .or_default()
                .push(version);
        }
        let mut by_major: BTreeMap<u64, Vec<Dependency>> = BTreeMap::new();
        for (major, group) in &by_major_refs {
            let first = group[0];
            for other in &group[1..] {
                ensure_same_dependency_graph(&first.model.dependencies, &other.model.dependencies)?;
            }
            by_major.insert(*major, first.model.dependencies.clone());
        }

        // Collect every `(ns, name)` seen in each major and detect conflicting definitions across
        // live majors.
        let mut defs_by_pkg: BTreeMap<(String, String), Vec<(u64, Dependency)>> = BTreeMap::new();
        for (&major, deps) in &by_major {
            for dep in deps {
                let key = (dep.package.namespace.clone(), dep.package.name.clone());
                defs_by_pkg
                    .entry(key)
                    .or_default()
                    .push((major, dep.clone()));
            }
        }

        let mut scoped_pkgs: BTreeSet<(String, String)> = BTreeSet::new();
        for (key, entries) in &defs_by_pkg {
            let first_major = entries[0].0;
            let differs = entries[1..].iter().any(|(other_major, _)| {
                !same_effective_dep_in_slices(
                    &by_major[&first_major],
                    &by_major[other_major],
                    &key.0,
                    &key.1,
                    &mut BTreeSet::new(),
                )
            });
            if differs {
                scoped_pkgs.insert(key.clone());
            }
        }
        // Any dependency (even one used by only a single live major) that references a scoped
        // dependency must also be scoped per major so its Rust/glue paths resolve inside
        // `witforge_deps::v<major>::*` rather than dangling from top-level `witforge_deps`.
        loop {
            let mut grew = false;
            for deps in by_major.values() {
                for dep in deps {
                    let key = (dep.package.namespace.clone(), dep.package.name.clone());
                    if scoped_pkgs.contains(&key) {
                        continue;
                    }
                    if dep_referenced_packages(dep)
                        .into_iter()
                        .any(|target| scoped_pkgs.contains(&target))
                    {
                        scoped_pkgs.insert(key);
                        grew = true;
                    }
                }
            }
            if !grew {
                break;
            }
        }

        // Compute canonical owners for scoped dependencies and disambiguation for colliding
        // `(ns, name, dep_major)` definitions across live majors.
        let mut canonical_owner: BTreeMap<(u64, String, String), u64> = BTreeMap::new();
        let mut disambiguated: BTreeSet<(u64, String, String)> = BTreeSet::new();
        for (key, entries) in &defs_by_pkg {
            if !scoped_pkgs.contains(key) {
                continue;
            }
            for (major, _) in entries {
                let owner = entries
                    .iter()
                    .find(|(cand_major, _)| {
                        same_effective_dep_in_slices(
                            &by_major[cand_major],
                            &by_major[major],
                            &key.0,
                            &key.1,
                            &mut BTreeSet::new(),
                        )
                    })
                    .map_or(*major, |(cand_major, _)| *cand_major);
                canonical_owner.insert((*major, key.0.clone(), key.1.clone()), owner);
            }
            let mut by_dep_major: BTreeMap<u64, Vec<u64>> = BTreeMap::new();
            for (root_major, dep) in entries {
                by_dep_major
                    .entry(dep.package.version.major)
                    .or_default()
                    .push(*root_major);
            }
            for group in by_dep_major.values() {
                if group.len() > 1 {
                    let first_major = group[0];
                    if group[1..].iter().any(|other_major| {
                        !same_effective_dep_in_slices(
                            &by_major[&first_major],
                            &by_major[other_major],
                            &key.0,
                            &key.1,
                            &mut BTreeSet::new(),
                        )
                    }) {
                        for root_major in group {
                            disambiguated.insert((*root_major, key.0.clone(), key.1.clone()));
                        }
                    }
                }
            }
        }
        // If a dependency references a disambiguated dependency in some major, disambiguate its
        // proto/schema files too so `import` / `$ref` targets stay consistent across majors.
        loop {
            let mut grew = false;
            for (&major, deps) in &by_major {
                for dep in deps {
                    let key = (dep.package.namespace.clone(), dep.package.name.clone());
                    if disambiguated.contains(&(major, key.0.clone(), key.1.clone())) {
                        continue;
                    }
                    let refs_disambig = dep_referenced_packages(dep)
                        .into_iter()
                        .any(|target| disambiguated.contains(&(major, target.0, target.1)));
                    if refs_disambig {
                        for (root_major, _) in defs_by_pkg.get(&key).into_iter().flatten() {
                            if disambiguated.insert((*root_major, key.0.clone(), key.1.clone())) {
                                grew = true;
                            }
                        }
                    }
                }
            }
            if !grew {
                break;
            }
        }

        let mut shared = BTreeMap::new();
        let mut per_major: BTreeMap<u64, Vec<Dependency>> = BTreeMap::new();
        for (&major, deps) in &by_major {
            for dep in deps {
                let key = (dep.package.namespace.clone(), dep.package.name.clone());
                if scoped_pkgs.contains(&key) {
                    per_major.entry(major).or_default().push(dep.clone());
                } else {
                    shared.entry(key).or_insert_with(|| dep.clone());
                }
            }
        }

        Ok(Self {
            shared,
            per_major,
            by_major,
            disambiguated,
            canonical_owner,
        })
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.shared.is_empty() && self.per_major.is_empty()
    }

    pub(crate) fn is_scoped(&self, namespace: &str, name: &str) -> bool {
        !self
            .shared
            .contains_key(&(namespace.to_string(), name.to_string()))
            && self.per_major.values().any(|deps| {
                deps.iter()
                    .any(|dep| dep.package.namespace == namespace && dep.package.name == name)
            })
    }

    pub(crate) fn canonical_owner_major(
        &self,
        root_major: u64,
        namespace: &str,
        name: &str,
    ) -> u64 {
        self.canonical_owner
            .get(&(root_major, namespace.to_string(), name.to_string()))
            .copied()
            .unwrap_or(root_major)
    }

    pub(crate) fn is_disambiguated(&self, root_major: u64, namespace: &str, name: &str) -> bool {
        self.disambiguated
            .contains(&(root_major, namespace.to_string(), name.to_string()))
    }

    pub(crate) fn proto_package(&self, root_major: u64, dep: &Dependency) -> String {
        let ns = ident::proto_package_segment(&dep.package.namespace);
        let name = ident::proto_package_segment(&dep.package.name);
        if self.is_disambiguated(root_major, &dep.package.namespace, &dep.package.name) {
            let owner =
                self.canonical_owner_major(root_major, &dep.package.namespace, &dep.package.name);
            format!("{ns}.{name}.v{}_m{owner}", dep.package.version.major)
        } else {
            format!("{ns}.{name}.v{}", dep.package.version.major)
        }
    }

    pub(crate) fn proto_file_name(&self, root_major: u64, dep: &Dependency) -> String {
        let ns = ident::proto_package_segment(&dep.package.namespace);
        let name = ident::proto_package_segment(&dep.package.name);
        if self.is_disambiguated(root_major, &dep.package.namespace, &dep.package.name) {
            let owner =
                self.canonical_owner_major(root_major, &dep.package.namespace, &dep.package.name);
            format!("v{owner}.{ns}.{name}.v{}.proto", dep.package.version.major)
        } else {
            format!("{ns}.{name}.v{}.proto", dep.package.version.major)
        }
    }

    pub(crate) fn prost_base(&self, proto_root: &str, root_major: u64, dep: &Dependency) -> String {
        let ns = ident::prost_package_segment(&dep.package.namespace);
        let name = ident::prost_package_segment(&dep.package.name);
        if self.is_disambiguated(root_major, &dep.package.namespace, &dep.package.name) {
            let owner =
                self.canonical_owner_major(root_major, &dep.package.namespace, &dep.package.name);
            format!(
                "{proto_root}::{ns}::{name}::v{}_m{owner}",
                dep.package.version.major
            )
        } else {
            format!("{proto_root}::{ns}::{name}::v{}", dep.package.version.major)
        }
    }

    pub(crate) fn schema_file_name(&self, root_major: u64, dep: &Dependency) -> String {
        let ns = dep.package.namespace.replace('-', "_");
        let name = dep.package.name.replace('-', "_");
        if self.is_disambiguated(root_major, &dep.package.namespace, &dep.package.name) {
            let owner =
                self.canonical_owner_major(root_major, &dep.package.namespace, &dep.package.name);
            format!(
                "v{owner}.{ns}.{name}.v{}.schema.json",
                dep.package.version.major
            )
        } else {
            format!("{ns}.{name}.v{}.schema.json", dep.package.version.major)
        }
    }
}

/// Whether two single-package dependency definitions are completely identical (same version, docs,
/// interfaces, types, functions, order, and metadata).
pub(crate) fn same_dependency(left: &Dependency, right: &Dependency) -> bool {
    dependency_graph_change(std::slice::from_ref(left), std::slice::from_ref(right)).is_none()
}

/// Whether `left` and `right` both pin `(namespace, name)` with an identical definition and an
/// identical transitive dependency subgraph.
pub(crate) fn same_effective_dependency(
    left: &Model,
    right: &Model,
    namespace: &str,
    name: &str,
) -> bool {
    same_effective_dep_in_slices(
        &left.dependencies,
        &right.dependencies,
        namespace,
        name,
        &mut BTreeSet::new(),
    )
}

fn same_effective_dep_in_slices(
    left_deps: &[Dependency],
    right_deps: &[Dependency],
    namespace: &str,
    name: &str,
    visited: &mut BTreeSet<(String, String)>,
) -> bool {
    if !visited.insert((namespace.to_string(), name.to_string())) {
        return true;
    }
    let Some(left) = left_deps
        .iter()
        .find(|dep| dep.package.namespace == namespace && dep.package.name == name)
    else {
        return false;
    };
    let Some(right) = right_deps
        .iter()
        .find(|dep| dep.package.namespace == namespace && dep.package.name == name)
    else {
        return false;
    };
    if !same_dependency(left, right) {
        return false;
    }
    for (tns, tpkg) in dep_referenced_packages(left) {
        if !same_effective_dep_in_slices(left_deps, right_deps, &tns, &tpkg, visited) {
            return false;
        }
    }
    true
}

fn dep_referenced_packages(dep: &Dependency) -> BTreeSet<(String, String)> {
    let mut out = BTreeSet::new();
    for iface in &dep.interfaces {
        for def in &iface.types {
            match &def.kind {
                TypeDefKind::Record(fields) => {
                    for field in fields {
                        collect_foreign_packages(&field.ty, &mut out);
                    }
                }
                TypeDefKind::Variant(cases) | TypeDefKind::Enum(cases) => {
                    for case in cases {
                        if let Some(payload) = &case.payload {
                            collect_foreign_packages(payload, &mut out);
                        }
                    }
                }
                TypeDefKind::Flags(_) => {}
                TypeDefKind::Alias(ty) | TypeDefKind::List(ty) | TypeDefKind::Option(ty) => {
                    collect_foreign_packages(ty, &mut out);
                }
                TypeDefKind::Result(res) => {
                    if let Some(ok) = &res.ok {
                        collect_foreign_packages(ok, &mut out);
                    }
                    if let Some(err) = &res.err {
                        collect_foreign_packages(err, &mut out);
                    }
                }
            }
        }
        for func in &iface.functions {
            for param in &func.params {
                collect_foreign_packages(&param.ty, &mut out);
            }
            if let Some(res) = &func.result {
                collect_foreign_packages(res, &mut out);
            }
        }
    }
    out.remove(&(dep.package.namespace.clone(), dep.package.name.clone()));
    out
}

fn collect_foreign_packages(ty: &crate::model::TypeExpr, out: &mut BTreeSet<(String, String)>) {
    match ty {
        crate::model::TypeExpr::Named(name) => {
            if let Some((ns, pkg)) = name.package() {
                out.insert((ns.to_string(), pkg.to_string()));
            }
        }
        crate::model::TypeExpr::List(inner) | crate::model::TypeExpr::Option(inner) => {
            collect_foreign_packages(inner, out);
        }
        crate::model::TypeExpr::Result(res) => {
            if let Some(ok) = &res.ok {
                collect_foreign_packages(ok, out);
            }
            if let Some(err) = &res.err {
                collect_foreign_packages(err, out);
            }
        }
        _ => {}
    }
}

/// Refuse two dependency graphs that cannot share one generated binding surface.
pub(crate) fn ensure_same_dependency_graph(old: &[Dependency], new: &[Dependency]) -> Result<()> {
    if let Some(change) = dependency_graph_change(old, new) {
        let message = format!(
            "dependency graph differs: {change}; every version in one major must use an \
             identical pinned dependency graph"
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
pub(crate) fn load_versions(wit_root: &Path) -> Result<Vec<Version>> {
    load_versions_with_released(wit_root, None)
}

#[allow(clippy::too_many_lines)]
pub(crate) fn load_versions_with_released(
    wit_root: &Path,
    explicit_released: Option<&Path>,
) -> Result<Vec<Version>> {
    let released = crate::load_root_released_manifest(wit_root, explicit_released)?;
    let discovered = discover_versions(wit_root)?;
    crate::validate_retirement_order(&discovered, &released, wit_root)?;
    let mut versions: Vec<Version> = Vec::new();
    let mut prev_lock_by_major: BTreeMap<u64, (PathBuf, WireLock)> = BTreeMap::new();
    let mut first_deps_by_major: BTreeMap<u64, Vec<Dependency>> = BTreeMap::new();
    let mut first_package: Option<(String, String)> = None;
    for (major, minor, directory) in discovered {
        let dir_name = directory
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .unwrap_or_default();
        let is_retired = released.is_retired(dir_name);
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
        if let Some((expected_ns, expected_name)) = &first_package {
            if expected_ns != &model.package.namespace || expected_name != &model.package.name {
                let mut diag = Diagnostic::error(
                    codes::PACKAGE_MISMATCH,
                    format!(
                        "{} declares package {}:{}, expected {expected_ns}:{expected_name}",
                        directory.display(),
                        model.package.namespace,
                        model.package.name,
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
        } else {
            first_package = Some((model.package.namespace.clone(), model.package.name.clone()));
        }
        let previous = prev_lock_by_major.get(&major);
        let lock_path = directory.join("wire.lock");
        let lock_text = read_version_lock(
            &directory,
            major,
            &lock_path,
            previous.map(|(dir, _)| dir.as_path()),
        )?;
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
        if let Some((prev_dir, prev_lock)) = previous {
            let previous_path = prev_dir.join("wire.lock");
            lock.check_append_only(prev_lock).map_err(|err| {
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
        let is_released = released.contains(dir_name);
        let drift = lock.drift(&members).map_err(|err| {
            let human = format!("in {}: {err:#}", lock_path.display());
            let lock_file = lock_path.display().to_string();
            let diag = Diagnostic::error(codes::LOCKFILE_ERROR, format!("{err:#}"))
                .with_path(lock_file.clone());
            let located = LocatedDiagnostic::from_diagnostic(diag).with_file(lock_file);
            ContractError::with_located_diagnostics(human, vec![located])
        })?;
        if !drift.is_empty() {
            let remedy = version_lock_remedy(
                &directory,
                previous.map(|(dir, _)| dir.as_path()),
                is_released,
            );
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
                    version_lock_remedy(
                        &directory,
                        previous.map(|(dir, _)| dir.as_path()),
                        is_released
                    )
                );
                let lock_file = lock_path.display().to_string();
                let diag =
                    Diagnostic::error(codes::LOCKFILE_ERROR, message).with_path(lock_file.clone());
                let located = LocatedDiagnostic::from_diagnostic(diag).with_file(lock_file);
                ContractError::with_located_diagnostic(located)
            })?;
        prev_lock_by_major.insert(major, (directory.clone(), lock.clone()));
        if let Some(baseline_deps) = first_deps_by_major.get(&major) {
            ensure_same_dependency_graph(baseline_deps, &model.dependencies)?;
        } else {
            first_deps_by_major.insert(major, model.dependencies.clone());
        }
        if !is_retired {
            versions.push(Version {
                major,
                directory,
                model,
                lock,
                locations,
            });
        }
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
    previous_dir: Option<&Path>,
) -> Result<String> {
    match fs::read_to_string(lock_path) {
        Ok(text) => Ok(text),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            let quoted_dir = crate::shell_quote(directory);
            let message = if let Some(prev_dir) = previous_dir {
                let prev_lock = prev_dir.join("wire.lock");
                format!(
                    "lockfile {} does not exist; major {major} already has {}, so copy {} to \
                     {} and run `witforge lock {} --against {}` (or for a new major, create a \
                     fresh lockfile with `--first`)",
                    lock_path.display(),
                    prev_dir.display(),
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

fn version_lock_remedy(directory: &Path, previous_dir: Option<&Path>, is_released: bool) -> String {
    if is_released {
        format!(
            "`{}` is released, so `witforge lock` refuses it; restore its released WIT and \
             wire.lock",
            directory.display()
        )
    } else if let Some(prev_dir) = previous_dir {
        let previous_path = prev_dir.join("wire.lock");
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
