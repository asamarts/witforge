//! The restricted WIT subset that every contract must stay inside.
//!
//! Version 1 of the subset is pure and synchronous, and maps cleanly onto Rust
//! and protobuf: no resources or handles, no `future`, `stream`, or `error-context`, no `map`, no
//! fixed-length lists, no `char`, no tuples, no `async` functions or `get`/`set` accessors.
//! Types and functions live in the package's named interfaces, and a local type may reference a
//! named type from a standard `deps/` package. Worlds may import and export interfaces defined
//! by this package or by a standard `deps/` package without renaming them or defining them
//! inline. Only `@since`, `@unstable`, and `@deprecated` annotations are accepted. Names in one
//! scope must be distinct ASCII case-insensitively. Anything the checker does not recognise is
//! refused rather than passed through, so a new `wit-parser` construct cannot slip into the
//! generated bindings.

use std::collections::{BTreeMap, HashSet, VecDeque};
use std::fmt;

use wit_parser::{
    Function, FunctionKind, InterfaceId, Type, TypeDefKind, TypeId, TypeOwner, World, WorldId,
    WorldItem, WorldKey,
};

use crate::Contract;
use crate::diagnostic::Diagnostic;
use crate::emit::ident;

/// Diagnostic code for a WIT subset violation.
const CODE_SUBSET: &str = "WF1001";
/// Diagnostic code for two names in one scope that collide (ASCII case-insensitive or generated
/// identifier collision).
const CODE_IDENTIFIER_COLLISION: &str = "WF1003";

/// One construct outside the subset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    /// Where the construct appears, for example `alint:plugin/types/finding.span`.
    pub location: String,
    /// Why it is refused, with the supported alternative.
    pub message: String,
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.location, self.message)
    }
}

/// Check the root package and its standard `deps/` dependencies against the subset.
#[must_use]
pub fn check(contract: &Contract) -> Vec<Violation> {
    check_diagnostics(contract)
        .into_iter()
        .map(|diagnostic| Violation {
            location: diagnostic.path.unwrap_or_default(),
            message: diagnostic.message,
        })
        .collect()
}

/// Check the root package and its standard `deps/` dependencies and return structured
/// diagnostics with their stable codes (`WF1001` for subset violations, `WF1003` for
/// case-insensitive scope collisions).
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn check_diagnostics(contract: &Contract) -> Vec<Diagnostic> {
    let resolve = &contract.resolve;
    let package = &resolve.packages[contract.package];
    let pkg = format!("{}:{}", package.name.namespace, package.name.name);
    let mut out = Vec::new();
    let mut required_dependency_types = HashSet::new();

    if package.name.version.is_none() {
        refuse(
            &mut out,
            &pkg,
            "the contract package needs a version (`package ns:name@x.y.0;`) so `@since` gates \
             and evolution checks are possible",
        );
    } else if let Some(version) = &package.name.version
        && (version.patch != 0
            || !version.pre.as_str().is_empty()
            || !version.build.as_str().is_empty())
    {
        refuse(
            &mut out,
            &pkg,
            "the contract version must be `<major>.<minor>.0`; patch, pre-release, and \
             build metadata are not supported",
        );
    }

    // A contract is one package. Any other package in the resolve comes from a nested
    // `package ns:name { ... }` block or a dependency, and nothing in it is locked or generated,
    // so it is refused rather than silently left out of the output.
    for (id, other) in &resolve.packages {
        if id != contract.package && !contract.is_dependency(id) {
            refuse(
                &mut out,
                &pkg,
                format!(
                    "the version directory also loads package `{}` from a nested `package` \
                     block; dependencies must live under the standard `deps/` directory",
                    other.name
                ),
            );
        }
    }

    // Every package under `deps/` contributes to the generated dependency surface, whether or
    // not the root references it, so every dependency package's version and interface scope are
    // checked here as subset diagnostics (exit 1).
    let mut dependencies = contract.dependencies.clone();
    dependencies.sort_by(|left, right| {
        let left_name = &resolve.packages[*left].name;
        let right_name = &resolve.packages[*right].name;
        (&left_name.namespace, &left_name.name).cmp(&(&right_name.namespace, &right_name.name))
    });
    let mut seen_packages: BTreeMap<String, (&str, String)> = BTreeMap::new();
    seen_packages.insert(pkg.to_ascii_lowercase(), ("root package", pkg.clone()));
    for dep_id in dependencies {
        let dep = &resolve.packages[dep_id];
        let dep_pkg = format!("{}:{}", dep.name.namespace, dep.name.name);
        let folded_pkg = dep_pkg.to_ascii_lowercase();
        if let Some((prev_kind, prev_name)) = seen_packages.get(&folded_pkg) {
            if prev_name != &dep_pkg {
                refuse_collision(
                    &mut out,
                    &dep_pkg,
                    format!(
                        "dependency package `{dep_pkg}` differs only in letter case from \
                         {prev_kind} `{prev_name}`; give them distinct names"
                    ),
                );
            }
        } else {
            seen_packages.insert(folded_pkg, ("dependency package", dep_pkg.clone()));
        }
        let entry = contract
            .dependency_entry(dep_id)
            .map_or_else(|| "deps/".to_string(), |path| path.display().to_string());
        match &dep.name.version {
            None => refuse(
                &mut out,
                &dep_pkg,
                format!(
                    "dependency package `{dep_pkg}` in `{entry}` has no version; dependency \
                     versions must be `<major>.<minor>.0`"
                ),
            ),
            Some(version)
                if version.patch != 0
                    || !version.pre.as_str().is_empty()
                    || !version.build.as_str().is_empty() =>
            {
                refuse(
                    &mut out,
                    &dep_pkg,
                    format!(
                        "dependency package `{dep_pkg}@{version}` in `{entry}` has unsupported \
                         version `{version}`; dependency versions must be `<major>.<minor>.0` \
                         (patch, pre-release, and build metadata are not supported)"
                    ),
                );
            }
            Some(_) => {}
        }
        check_scope_case_collisions(
            &dep_pkg,
            "interface",
            dep.interfaces.keys().map(String::as_str),
            |name| format!("{dep_pkg}/{name}"),
            &mut out,
        );
        check_package_generated_collisions(resolve, &dep_pkg, dep, &mut out);
    }

    // Package scope: interfaces and worlds share one WIT package namespace; names differing only
    // in ASCII letter case map to one generated identifier and are refused.
    check_package_case_collisions(&pkg, package, &mut out);
    check_package_generated_collisions(resolve, &pkg, package, &mut out);

    for (iface_name, &iface_id) in &package.interfaces {
        let iface = &resolve.interfaces[iface_id];
        check_interface_case_collisions(&pkg, iface_name, iface, &mut out);
        // `use` aliases are owned by the interface that names them, so they are checked here
        // too: an alias of a type from another package is refused at the `use`.
        for (type_name, &type_id) in &iface.types {
            let at = format!("{pkg}/{iface_name}/{type_name}");
            check_external_id(
                resolve.types[type_id].external_id.as_deref(),
                &at,
                None,
                &mut out,
            );
            check_def(contract, type_id, &at, None, &mut out);
            check_dependency_def(
                contract,
                type_id,
                &at,
                &mut required_dependency_types,
                None,
                &mut out,
            );
        }
        for (func_name, func) in &iface.functions {
            let at = format!("{pkg}/{iface_name}#{func_name}");
            check_external_id(func.external_id.as_deref(), &at, None, &mut out);
            check_function(contract, func, &at, None, &mut out);
            check_dependency_function_refs(
                contract,
                func,
                &at,
                &mut required_dependency_types,
                None,
                &mut out,
            );
        }
    }

    for (world_name, &world_id) in &package.worlds {
        check_world(
            contract,
            world_id,
            &format!("{pkg}/{world_name}"),
            &mut required_dependency_types,
            &mut out,
        );
    }
    out
}

fn check_package_case_collisions(
    pkg: &str,
    package: &wit_parser::Package,
    out: &mut Vec<Diagnostic>,
) {
    let mut seen: BTreeMap<String, (&str, &str)> = BTreeMap::new();
    let items = package
        .interfaces
        .keys()
        .map(|name| ("interface", name.as_str()))
        .chain(package.worlds.keys().map(|name| ("world", name.as_str())));
    for (kind, name) in items {
        let folded = name.to_ascii_lowercase();
        if let Some((prev_kind, prev_name)) = seen.get(&folded) {
            if *prev_name != name {
                refuse_collision(
                    out,
                    format!("{pkg}/{name}"),
                    format!(
                        "{kind} `{name}` differs only in letter case from {prev_kind} \
                         `{prev_name}` in `{pkg}`; give them distinct names"
                    ),
                );
            }
        } else {
            seen.insert(folded, (kind, name));
        }
    }
}

fn check_interface_case_collisions(
    pkg: &str,
    iface_name: &str,
    iface: &wit_parser::Interface,
    out: &mut Vec<Diagnostic>,
) {
    let scope = format!("{pkg}/{iface_name}");
    let mut seen: BTreeMap<String, (&str, &str)> = BTreeMap::new();
    let items = iface
        .types
        .keys()
        .map(|name| ("type", name.as_str()))
        .chain(
            iface
                .functions
                .keys()
                .map(|name| ("function", name.as_str())),
        );
    for (kind, name) in items {
        let folded = name.to_ascii_lowercase();
        if let Some((prev_kind, prev_name)) = seen.get(&folded) {
            if *prev_name != name {
                let location = if kind == "function" {
                    format!("{scope}#{name}")
                } else {
                    format!("{scope}/{name}")
                };
                refuse_collision(
                    out,
                    location,
                    format!(
                        "{kind} `{name}` differs only in letter case from {prev_kind} \
                         `{prev_name}` in `{scope}`; give them distinct names"
                    ),
                );
            }
        } else {
            seen.insert(folded, (kind, name));
        }
    }
}

fn check_scope_case_collisions<'a>(
    scope: &str,
    kind: &str,
    names: impl Iterator<Item = &'a str>,
    location: impl Fn(&str) -> String,
    out: &mut Vec<Diagnostic>,
) {
    let mut seen: BTreeMap<String, &str> = BTreeMap::new();
    for name in names {
        let folded = name.to_ascii_lowercase();
        if let Some(&prev) = seen.get(&folded) {
            if prev != name {
                refuse_collision(
                    out,
                    location(name),
                    format!(
                        "{kind} `{name}` differs only in letter case from {kind} `{prev}` in \
                         `{scope}`; give them distinct names"
                    ),
                );
            }
        } else {
            seen.insert(folded, name);
        }
    }
}

fn check_external_id(
    external_id: Option<&str>,
    at: &str,
    reason: Option<&str>,
    out: &mut Vec<Diagnostic>,
) {
    if let Some(id) = external_id {
        refuse_with_reason(
            out,
            at,
            format!(
                "annotation `@external-id(\"{id}\")` is not supported; only `@since`, \
                 `@unstable`, and `@deprecated` are accepted"
            ),
            reason,
        );
    }
}

#[allow(clippy::too_many_lines)]
fn check_world(
    contract: &Contract,
    world_id: WorldId,
    at: &str,
    required_dependency_types: &mut HashSet<TypeId>,
    out: &mut Vec<Diagnostic>,
) {
    let resolve = &contract.resolve;
    let world = &resolve.worlds[world_id];
    for (verb, singular, items) in [
        ("imports", "import", &world.imports),
        ("exports", "export", &world.exports),
    ] {
        for (key, item) in items {
            match (key, item) {
                (
                    WorldKey::Name(name),
                    WorldItem::Interface {
                        id, external_id, ..
                    },
                ) => {
                    let item_at = format!("{at}/{name}");
                    check_external_id(external_id.as_deref(), &item_at, None, out);
                    let iface = &resolve.interfaces[*id];
                    if let Some(iface_name) = iface.name.as_deref() {
                        let target = if iface.package == Some(contract.package) {
                            iface_name.to_string()
                        } else {
                            resolve.id_of(*id).unwrap_or_else(|| iface_name.to_string())
                        };
                        refuse(
                            out,
                            item_at,
                            format!(
                                "named interface {singular}s are not supported \
                                 (`{singular} {name}: {target}`); {singular} `{target}` directly \
                                 without a local name"
                            ),
                        );
                    } else {
                        refuse(
                            out,
                            item_at,
                            "inline interfaces are not supported; define a named interface in the \
                             package and import or export it",
                        );
                    }
                }
                (
                    WorldKey::Interface(_),
                    WorldItem::Interface {
                        id, external_id, ..
                    },
                ) => {
                    let is_implicit = singular == "import"
                        && !contract.is_explicit_world_import(&world.name, *id);
                    if !is_implicit {
                        let label = interface_display_name(contract, *id);
                        check_external_id(
                            external_id.as_deref(),
                            &format!("{at}/{label}"),
                            None,
                            out,
                        );
                    }
                    let iface = &resolve.interfaces[*id];
                    if let Some(package) = iface.package {
                        if package != contract.package {
                            if contract.is_dependency(package) {
                                let reason = if is_implicit {
                                    implicit_import_reason(contract, world, *id)
                                } else {
                                    let label = interface_display_name(contract, *id);
                                    format!(
                                        "required by world `{}` {singular} `{label}`",
                                        world.name
                                    )
                                };
                                check_dependency_interface(
                                    contract,
                                    *id,
                                    required_dependency_types,
                                    Some(&reason),
                                    out,
                                );
                            } else {
                                // `id_of` panics on an interface without a package.
                                let name = resolve.id_of(*id).unwrap_or_default();
                                refuse(
                                    out,
                                    at,
                                    format!(
                                        "{verb} `{name}` from a package outside the standard \
                                         `deps/` directory"
                                    ),
                                );
                            }
                        }
                    } else {
                        refuse(out, at, format!("{verb} an interface with no package"));
                    }
                }
                (_, WorldItem::Function(func)) => {
                    let item_at = format!("{at}#{}", func.name);
                    check_external_id(func.external_id.as_deref(), &item_at, None, out);
                    refuse(
                        out,
                        item_at,
                        "functions must be defined in an interface; export the interface from the \
                         world",
                    );
                }
                (_, WorldItem::Type { id, .. }) => {
                    let type_name = resolve.types[*id].name.as_deref().unwrap_or_default();
                    check_external_id(
                        resolve.types[*id].external_id.as_deref(),
                        &format!("{at}/{type_name}"),
                        None,
                        out,
                    );
                    check_world_type(contract, world_id, *id, at, out);
                    check_dependency_def(contract, *id, at, required_dependency_types, None, out);
                }
            }
        }
    }
}

fn interface_display_name(contract: &Contract, id: InterfaceId) -> String {
    let resolve = &contract.resolve;
    let iface = &resolve.interfaces[id];
    let name = iface.name.as_deref().unwrap_or_default();
    match iface.package {
        Some(pkg) if pkg != contract.package => {
            let package = &resolve.packages[pkg].name;
            format!("{}:{}/{name}", package.namespace, package.name)
        }
        _ => name.to_string(),
    }
}

/// Explain which world item caused `wit-parser` to implicitly import dependency `target`.
fn implicit_import_reason(contract: &Contract, world: &World, target: InterfaceId) -> String {
    let resolve = &contract.resolve;
    for (key, item) in &world.exports {
        if let WorldItem::Interface { id, .. } = item
            && interface_reaches(resolve, *id, target)
        {
            let label = world_interface_label(contract, key, *id);
            return format!(
                "implicitly imported by world `{}` via exported interface `{label}`",
                world.name
            );
        }
    }
    for (key, item) in &world.imports {
        let WorldItem::Interface { id, .. } = item else {
            continue;
        };
        if *id == target {
            continue;
        }
        let is_explicit = match key {
            WorldKey::Name(_) => true,
            WorldKey::Interface(_) => contract.is_explicit_world_import(&world.name, *id),
        };
        if is_explicit && interface_reaches(resolve, *id, target) {
            let label = world_interface_label(contract, key, *id);
            return format!(
                "implicitly imported by world `{}` via imported interface `{label}`",
                world.name
            );
        }
    }
    for (key, item) in &world.imports {
        if let WorldItem::Type { id, .. } = item
            && type_reaches_interface(resolve, *id, target)
        {
            let type_name = match key {
                WorldKey::Name(name) => name.as_str(),
                WorldKey::Interface(_) => resolve.types[*id].name.as_deref().unwrap_or_default(),
            };
            return format!(
                "implicitly imported by world `{}` via world type `{type_name}`",
                world.name
            );
        }
    }
    for (ns, pkg_name, foreign_world_name) in contract.included_foreign_worlds(&world.name) {
        if foreign_world_reaches_interface(resolve, ns, pkg_name, foreign_world_name, target) {
            return format!(
                "implicitly imported by world `{}` via included world \
                 `{ns}:{pkg_name}/{foreign_world_name}`",
                world.name
            );
        }
    }
    format!("implicitly imported by world `{}`", world.name)
}

fn world_interface_label(contract: &Contract, key: &WorldKey, id: InterfaceId) -> String {
    match key {
        WorldKey::Name(name) => name.clone(),
        WorldKey::Interface(_) => interface_display_name(contract, id),
    }
}

fn foreign_world_reaches_interface(
    resolve: &wit_parser::Resolve,
    namespace: &str,
    package_name: &str,
    world_name: &str,
    target: InterfaceId,
) -> bool {
    let Some((_, package)) = resolve
        .packages
        .iter()
        .find(|(_, pkg)| pkg.name.namespace == namespace && pkg.name.name == package_name)
    else {
        return false;
    };
    let Some(&world_id) = package.worlds.get(world_name) else {
        return false;
    };
    let world = &resolve.worlds[world_id];
    world
        .imports
        .values()
        .chain(world.exports.values())
        .any(|item| match item {
            WorldItem::Interface { id, .. } => {
                *id == target || interface_reaches(resolve, *id, target)
            }
            WorldItem::Type { id, .. } => type_reaches_interface(resolve, *id, target),
            WorldItem::Function(_) => false,
        })
}

fn interface_reaches(
    resolve: &wit_parser::Resolve,
    start: InterfaceId,
    target: InterfaceId,
) -> bool {
    let mut visited = HashSet::new();
    let mut queue = VecDeque::new();
    for dep in resolve.interface_direct_deps(start) {
        if visited.insert(dep) {
            queue.push_back(dep);
        }
    }
    while let Some(current) = queue.pop_front() {
        if current == target {
            return true;
        }
        for dep in resolve.interface_direct_deps(current) {
            if visited.insert(dep) {
                queue.push_back(dep);
            }
        }
    }
    false
}

fn type_reaches_interface(
    resolve: &wit_parser::Resolve,
    type_id: TypeId,
    target: InterfaceId,
) -> bool {
    let mut visited = HashSet::new();
    let mut current = type_id;
    while visited.insert(current) {
        let def = &resolve.types[current];
        if let TypeOwner::Interface(iface_id) = def.owner {
            return iface_id == target || interface_reaches(resolve, iface_id, target);
        }
        if let TypeDefKind::Type(Type::Id(next)) = def.kind {
            current = next;
        } else {
            break;
        }
    }
    false
}

/// Validate every item exposed by a dependency interface and every named type it reaches.
///
/// Unsupported items in an otherwise unused dependency remain ignorable. Importing or exporting
/// the interface from a root world makes its complete API part of the root contract, so dropping
/// one of those items would silently generate a different interface.
fn check_dependency_interface(
    contract: &Contract,
    interface_id: InterfaceId,
    visited: &mut HashSet<TypeId>,
    reason: Option<&str>,
    out: &mut Vec<Diagnostic>,
) {
    let resolve = &contract.resolve;
    let interface = &resolve.interfaces[interface_id];
    let Some(package_id) = interface.package else {
        return;
    };
    let package = &resolve.packages[package_id];
    let dep_pkg = format!("{}:{}", package.name.namespace, package.name.name);
    let interface_name = interface.name.as_deref().unwrap_or_default();
    check_interface_case_collisions(&dep_pkg, interface_name, interface, out);
    let base = format!("{dep_pkg}/{interface_name}");
    for (name, id) in &interface.types {
        check_dependency_def(
            contract,
            *id,
            &format!("{base}/{name}"),
            visited,
            reason,
            out,
        );
    }
    for (name, function) in &interface.functions {
        let at = format!("{base}#{name}");
        check_external_id(function.external_id.as_deref(), &at, reason, out);
        check_function(contract, function, &at, reason, out);
        check_dependency_function_refs(contract, function, &at, visited, reason, out);
    }
}

fn check_dependency_function_refs(
    contract: &Contract,
    function: &Function,
    at: &str,
    visited: &mut HashSet<TypeId>,
    reason: Option<&str>,
    out: &mut Vec<Diagnostic>,
) {
    for param in &function.params {
        check_dependency_type_refs(
            contract,
            &param.ty,
            &format!("{at}({})", param.name),
            visited,
            reason,
            out,
        );
    }
    if let Some(result) = &function.result {
        check_dependency_type_refs(
            contract,
            result,
            &format!("{at} -> result"),
            visited,
            reason,
            out,
        );
    }
}

fn check_dependency_def(
    contract: &Contract,
    id: TypeId,
    at: &str,
    visited: &mut HashSet<TypeId>,
    reason: Option<&str>,
    out: &mut Vec<Diagnostic>,
) {
    if !visited.insert(id) {
        return;
    }
    if is_dependency_def(contract, id) {
        check_external_id(
            contract.resolve.types[id].external_id.as_deref(),
            at,
            reason,
            out,
        );
        check_def(contract, id, at, reason, out);
    }
    check_dependency_def_refs(contract, id, at, visited, reason, out);
}

fn check_dependency_def_refs(
    contract: &Contract,
    id: TypeId,
    at: &str,
    visited: &mut HashSet<TypeId>,
    reason: Option<&str>,
    out: &mut Vec<Diagnostic>,
) {
    match &contract.resolve.types[id].kind {
        TypeDefKind::Record(record) => {
            for field in &record.fields {
                check_dependency_type_refs(
                    contract,
                    &field.ty,
                    &format!("{at}.{}", field.name),
                    visited,
                    reason,
                    out,
                );
            }
        }
        TypeDefKind::Variant(variant) => {
            for case in &variant.cases {
                if let Some(ty) = &case.ty {
                    check_dependency_type_refs(
                        contract,
                        ty,
                        &format!("{at}.{}", case.name),
                        visited,
                        reason,
                        out,
                    );
                }
            }
        }
        TypeDefKind::Option(ty) | TypeDefKind::List(ty) | TypeDefKind::Type(ty) => {
            check_dependency_type_refs(contract, ty, at, visited, reason, out);
        }
        TypeDefKind::Result(result) => {
            for ty in [&result.ok, &result.err].into_iter().flatten() {
                check_dependency_type_refs(contract, ty, at, visited, reason, out);
            }
        }
        TypeDefKind::Enum(_)
        | TypeDefKind::Flags(_)
        | TypeDefKind::Tuple(_)
        | TypeDefKind::Resource
        | TypeDefKind::Handle(_)
        | TypeDefKind::Map(..)
        | TypeDefKind::FixedLengthList(..)
        | TypeDefKind::Future(_)
        | TypeDefKind::Stream(_)
        | TypeDefKind::Unknown => {}
    }
}

fn check_dependency_type_refs(
    contract: &Contract,
    ty: &Type,
    at: &str,
    visited: &mut HashSet<TypeId>,
    reason: Option<&str>,
    out: &mut Vec<Diagnostic>,
) {
    let Type::Id(id) = ty else {
        return;
    };
    let def = &contract.resolve.types[*id];
    if def.name.is_some() {
        let location = is_dependency_def(contract, *id)
            .then(|| dependency_type_location(contract, *id))
            .flatten();
        check_dependency_def(
            contract,
            *id,
            location.as_deref().unwrap_or(at),
            visited,
            reason,
            out,
        );
    } else {
        check_dependency_def_refs(contract, *id, at, visited, reason, out);
    }
}

fn is_dependency_def(contract: &Contract, id: TypeId) -> bool {
    let TypeOwner::Interface(interface_id) = contract.resolve.types[id].owner else {
        return false;
    };
    contract.resolve.interfaces[interface_id]
        .package
        .is_some_and(|package| contract.is_dependency(package))
}

fn dependency_type_location(contract: &Contract, id: TypeId) -> Option<String> {
    let resolve = &contract.resolve;
    let def = &resolve.types[id];
    let name = def.name.as_deref()?;
    let TypeOwner::Interface(interface_id) = def.owner else {
        return None;
    };
    let interface = &resolve.interfaces[interface_id];
    let package = &resolve.packages[interface.package?];
    Some(format!(
        "{}:{}/{}/{}",
        package.name.namespace,
        package.name.name,
        interface.name.as_deref()?,
        name
    ))
}

fn check_world_type(
    contract: &Contract,
    world_id: WorldId,
    id: TypeId,
    at: &str,
    out: &mut Vec<Diagnostic>,
) {
    let resolve = &contract.resolve;
    let def = &resolve.types[id];
    let at = format!("{at}/{}", def.name.as_deref().unwrap_or_default());
    match &def.kind {
        // Brought in by `include`: checked where it is defined, or refused if that is in
        // another package.
        _ if def.owner != TypeOwner::World(world_id) => {
            check_type(contract, &Type::Id(id), &at, None, out);
        }
        // `use` in a world names a type defined in an interface; it defines nothing itself.
        TypeDefKind::Type(target @ Type::Id(target_id))
            if resolve.types[*target_id].name.is_some() =>
        {
            check_type(contract, target, &at, None, out);
        }
        _ => refuse(
            out,
            at,
            "types must be defined in an interface, not directly in a world",
        ),
    }
}

fn check_function(
    contract: &Contract,
    func: &Function,
    at: &str,
    reason: Option<&str>,
    out: &mut Vec<Diagnostic>,
) {
    match func.kind {
        FunctionKind::Freestanding => {
            check_scope_case_collisions(
                at,
                "parameter",
                func.params.iter().map(|param| param.name.as_str()),
                |name| format!("{at}({name})"),
                out,
            );
            for param in &func.params {
                check_type(
                    contract,
                    &param.ty,
                    &format!("{at}({})", param.name),
                    reason,
                    out,
                );
            }
            if let Some(result) = &func.result {
                check_type(contract, result, &format!("{at} -> result"), reason, out);
            }
        }
        FunctionKind::AsyncFreestanding => refuse_with_reason(
            out,
            at,
            "`async` functions are not supported; the contract is synchronous",
            reason,
        ),
        FunctionKind::Getter | FunctionKind::Setter => refuse_with_reason(
            out,
            at,
            "`get` and `set` accessors are not supported; use a function",
            reason,
        ),
        // A resource's functions: the resource itself is refused where it is defined.
        FunctionKind::Method(_)
        | FunctionKind::AsyncMethod(_)
        | FunctionKind::MethodGetter(_)
        | FunctionKind::MethodSetter(_)
        | FunctionKind::Static(_)
        | FunctionKind::AsyncStatic(_)
        | FunctionKind::StaticGetter(_)
        | FunctionKind::StaticSetter(_)
        | FunctionKind::Constructor(_) => {}
    }
}

#[allow(clippy::too_many_lines)]
fn check_def(
    contract: &Contract,
    id: TypeId,
    at: &str,
    reason: Option<&str>,
    out: &mut Vec<Diagnostic>,
) {
    match &contract.resolve.types[id].kind {
        TypeDefKind::Record(r) => {
            // `wit-parser` already rejects empty records; kept in case a bump stops doing so.
            if r.fields.is_empty() {
                refuse_with_reason(
                    out,
                    at,
                    "empty records are not supported; add a field or use a variant case without payload",
                    reason,
                );
            }
            check_scope_case_collisions(
                at,
                "field",
                r.fields.iter().map(|field| field.name.as_str()),
                |name| format!("{at}.{name}"),
                out,
            );
            for field in &r.fields {
                check_type(
                    contract,
                    &field.ty,
                    &format!("{at}.{}", field.name),
                    reason,
                    out,
                );
            }
        }
        TypeDefKind::Variant(v) => {
            check_scope_case_collisions(
                at,
                "case",
                v.cases.iter().map(|case| case.name.as_str()),
                |name| format!("{at}.{name}"),
                out,
            );
            for case in &v.cases {
                if let Some(ty) = &case.ty {
                    check_type(contract, ty, &format!("{at}.{}", case.name), reason, out);
                }
            }
        }
        TypeDefKind::Enum(e) => {
            check_scope_case_collisions(
                at,
                "case",
                e.cases.iter().map(|case| case.name.as_str()),
                |name| format!("{at}.{name}"),
                out,
            );
        }
        TypeDefKind::Flags(f) => {
            check_scope_case_collisions(
                at,
                "flag",
                f.flags.iter().map(|flag| flag.name.as_str()),
                |name| format!("{at}.{name}"),
                out,
            );
        }
        TypeDefKind::Option(ty) | TypeDefKind::List(ty) | TypeDefKind::Type(ty) => {
            check_type(contract, ty, at, reason, out);
        }
        TypeDefKind::Result(r) => {
            for ty in [&r.ok, &r.err].into_iter().flatten() {
                check_type(contract, ty, at, reason, out);
            }
        }
        TypeDefKind::Tuple(_) => {
            refuse_with_reason(
                out,
                at,
                "tuples are not supported; use a named record",
                reason,
            );
        }
        TypeDefKind::Resource | TypeDefKind::Handle(_) => refuse_with_reason(
            out,
            at,
            "resources and handles are not supported; contract operations are pure functions over values",
            reason,
        ),
        TypeDefKind::Map(..) => {
            refuse_with_reason(
                out,
                at,
                "`map` is not supported yet; use `list<record>`",
                reason,
            );
        }
        TypeDefKind::FixedLengthList(..) => {
            refuse_with_reason(
                out,
                at,
                "fixed-length lists are not supported; use `list`",
                reason,
            );
        }
        TypeDefKind::Future(_) | TypeDefKind::Stream(_) => {
            refuse_with_reason(
                out,
                at,
                "`future` and `stream` are not supported; the contract is synchronous",
                reason,
            );
        }
        TypeDefKind::Unknown => refuse_with_reason(out, at, "unresolved type", reason),
    }
}

fn check_type(
    contract: &Contract,
    ty: &Type,
    at: &str,
    reason: Option<&str>,
    out: &mut Vec<Diagnostic>,
) {
    match ty {
        // Supported primitives. Listed explicitly so a new `wit-parser` type variant is a
        // compile error rather than silently passing the subset.
        Type::Bool
        | Type::U8
        | Type::U16
        | Type::U32
        | Type::U64
        | Type::S8
        | Type::S16
        | Type::S32
        | Type::S64
        | Type::F32
        | Type::F64
        | Type::String => {}
        Type::Char => {
            refuse_with_reason(out, at, "`char` is not supported; use `string`", reason);
        }
        Type::ErrorContext => {
            refuse_with_reason(out, at, "`error-context` is not supported", reason);
        }
        Type::Id(id) => {
            let def = &contract.resolve.types[*id];
            // Anonymous types (`option<T>`, `list<T>`, `result<..>`, tuples) are checked
            // where they are used. A named dependency type is resolved by wit-parser through
            // the standard deps layout; its own package is responsible for its subset and the
            // model records the qualified package identity.
            if def.name.is_none() {
                check_def(contract, *id, at, reason, out);
            }
        }
    }
}

fn refuse(out: &mut Vec<Diagnostic>, location: impl Into<String>, message: impl Into<String>) {
    out.push(Diagnostic::error(CODE_SUBSET, message).with_path(location));
}

fn refuse_with_reason(
    out: &mut Vec<Diagnostic>,
    location: impl Into<String>,
    message: impl Into<String>,
    reason: Option<&str>,
) {
    let message = match reason {
        Some(reason) => format!("{} ({reason})", message.into()),
        None => message.into(),
    };
    refuse(out, location, message);
}

fn refuse_collision(
    out: &mut Vec<Diagnostic>,
    location: impl Into<String>,
    message: impl Into<String>,
) {
    out.push(Diagnostic::error(CODE_IDENTIFIER_COLLISION, message).with_path(location));
}

/// Detect pairs of distinct WIT names in one package that map to one generated Rust, protobuf, or
/// `prost` identifier.
#[allow(clippy::too_many_lines)]
fn check_package_generated_collisions(
    resolve: &wit_parser::Resolve,
    pkg: &str,
    package: &wit_parser::Package,
    out: &mut Vec<Diagnostic>,
) {
    let mut rust_mods: BTreeMap<String, String> = BTreeMap::new();
    let mut rust_traits: BTreeMap<String, String> = BTreeMap::new();
    let mut proto_types: BTreeMap<String, String> = BTreeMap::new();
    let mut prost_types: BTreeMap<String, String> = BTreeMap::new();
    let mut prost_oneof_mods: BTreeMap<String, String> = BTreeMap::new();
    let mut proto_enum_vals: BTreeMap<String, String> = BTreeMap::new();

    for (iface_name, &iface_id) in &package.interfaces {
        let iface = &resolve.interfaces[iface_id];
        if let Ok(module) = ident::plain_ident(&ident::plain_snake(iface_name), "interface module")
            && let Some(prev) = rust_mods.insert(module.clone(), iface_name.clone())
            && !prev.eq_ignore_ascii_case(iface_name)
        {
            refuse_collision(
                out,
                format!("{pkg}/{iface_name}"),
                format!(
                    "interfaces `{prev}` and `{iface_name}` both map to Rust module `{module}`"
                ),
            );
        }
        if let Ok(trait_name) =
            ident::plain_ident(&ident::plain_pascal(iface_name), "interface trait")
            && let Some(prev) = rust_traits.insert(trait_name.clone(), iface_name.clone())
            && !prev.eq_ignore_ascii_case(iface_name)
        {
            refuse_collision(
                out,
                format!("{pkg}/{iface_name}"),
                format!(
                    "interfaces `{prev}` and `{iface_name}` both map to Rust trait `{trait_name}`"
                ),
            );
        }

        // Method enum values inside one interface and across the package.
        let method_enum = ident::proto_method_enum_name(iface_name);
        let method_zero = ident::proto_method_enum_zero_value(iface_name);
        let mut method_prost_variants: BTreeMap<String, String> = BTreeMap::new();
        method_prost_variants.insert(
            ident::prost_enum_variant(&method_enum, &method_zero),
            method_zero.clone(),
        );
        if let Some(prev) =
            proto_enum_vals.insert(method_zero.clone(), format!("method enum {iface_name}"))
        {
            refuse_collision(
                out,
                format!("{pkg}/{iface_name}"),
                format!(
                    "proto enum value `{method_zero}` is produced by both {prev} and method enum {iface_name}"
                ),
            );
        }
        for (func_name, func) in &iface.functions {
            if !matches!(func.kind, FunctionKind::Freestanding) {
                continue;
            }
            let val = ident::proto_method_enum_value(iface_name, func_name);
            let source = format!("function {iface_name}#{func_name}");
            if let Some(prev) = proto_enum_vals.insert(val.clone(), source.clone()) {
                refuse_collision(
                    out,
                    format!("{pkg}/{iface_name}#{func_name}"),
                    format!("proto enum value `{val}` is produced by both {prev} and {source}"),
                );
            }
            let prost_var = ident::prost_enum_variant(&method_enum, &val);
            if let Some(prev) = method_prost_variants.insert(prost_var.clone(), val.clone()) {
                refuse_collision(
                    out,
                    format!("{pkg}/{iface_name}#{func_name}"),
                    format!(
                        "method values `{prev}` and `{val}` in `{iface_name}` both map to prost variant `{prost_var}`"
                    ),
                );
            }
            check_prost_field_accessor_collisions(
                resolve,
                &format!("{pkg}/{iface_name}#{func_name}"),
                &format!("function `{iface_name}#{func_name}`"),
                "parameter",
                func.params
                    .iter()
                    .map(|param| (param.name.as_str(), &param.ty)),
                |name| format!("{pkg}/{iface_name}#{func_name}({name})"),
                out,
            );
        }

        // Named types defined by this interface (skip `use` aliases, which emit no proto message).
        for (type_name, &type_id) in &iface.types {
            let def = &resolve.types[type_id];
            if def.owner != TypeOwner::Interface(iface_id) {
                continue;
            }
            let produces_proto_type = matches!(
                def.kind,
                TypeDefKind::Record(_)
                    | TypeDefKind::Variant(_)
                    | TypeDefKind::Enum(_)
                    | TypeDefKind::Flags(_)
                    | TypeDefKind::Result(_)
            );
            if !produces_proto_type {
                continue;
            }
            let proto_name = ident::proto_type_name(iface_name, type_name);
            let prost_name = ident::prost_type_ident(&proto_name);
            let source = format!("type {iface_name}/{type_name}");
            let location = format!("{pkg}/{iface_name}/{type_name}");
            if let Some(prev) = proto_types.insert(proto_name.clone(), source.clone()) {
                refuse_collision(
                    out,
                    &location,
                    format!("proto name `{proto_name}` is produced by both {prev} and {source}"),
                );
            } else if let Some(prev) = prost_types.insert(prost_name.clone(), source.clone()) {
                refuse_collision(
                    out,
                    &location,
                    format!(
                        "prost type name `{prost_name}` is produced by both {prev} and {source}"
                    ),
                );
            }
            if matches!(def.kind, TypeDefKind::Variant(_) | TypeDefKind::Result(_)) {
                let oneof_mod = ident::prost_oneof_module(&proto_name);
                if let Some(prev) = prost_oneof_mods.insert(oneof_mod.clone(), source.clone()) {
                    refuse_collision(
                        out,
                        &location,
                        format!(
                            "prost oneof module `{oneof_mod}` is produced by both {prev} and {source}"
                        ),
                    );
                }
            }
            if let TypeDefKind::Record(record) = &def.kind {
                check_prost_field_accessor_collisions(
                    resolve,
                    &location,
                    &format!("record `{iface_name}/{type_name}`"),
                    "field",
                    record
                        .fields
                        .iter()
                        .map(|field| (field.name.as_str(), &field.ty)),
                    |name| format!("{location}.{name}"),
                    out,
                );
            }
            if let TypeDefKind::Enum(enum_def) = &def.kind {
                let zero = ident::proto_enum_zero_value(&proto_name);
                let mut enum_prost_variants: BTreeMap<String, String> = BTreeMap::new();
                enum_prost_variants
                    .insert(ident::prost_enum_variant(&proto_name, &zero), zero.clone());
                if let Some(prev) = proto_enum_vals.insert(zero.clone(), source.clone()) {
                    refuse_collision(
                        out,
                        &location,
                        format!(
                            "proto enum value `{zero}` is produced by both {prev} and {source}"
                        ),
                    );
                }
                for case in &enum_def.cases {
                    let val = ident::proto_enum_value_name(&proto_name, &case.name);
                    let case_source = format!("enum {iface_name}/{type_name}.{}", case.name);
                    if let Some(prev) = proto_enum_vals.insert(val.clone(), case_source.clone()) {
                        refuse_collision(
                            out,
                            format!("{location}.{}", case.name),
                            format!(
                                "proto enum value `{val}` is produced by both {prev} and {case_source}"
                            ),
                        );
                    }
                    let prost_var = ident::prost_enum_variant(&proto_name, &val);
                    if let Some(prev) = enum_prost_variants.insert(prost_var.clone(), val.clone()) {
                        refuse_collision(
                            out,
                            format!("{location}.{}", case.name),
                            format!(
                                "enum `{iface_name}/{type_name}`: proto values `{prev}` and `{val}` both map to prost variant `{prost_var}`"
                            ),
                        );
                    }
                }
            }
        }
    }
}

/// Detect collisions between inherent accessor methods generated by `prost-derive` 0.14 on a
/// single protobuf message (record or function request).
fn check_prost_field_accessor_collisions<'a>(
    resolve: &wit_parser::Resolve,
    _parent_location: &str,
    parent_label: &str,
    member_kind: &str,
    fields: impl IntoIterator<Item = (&'a str, &'a Type)>,
    location_for: impl Fn(&str) -> String,
    out: &mut Vec<Diagnostic>,
) {
    let mut accessors: BTreeMap<String, String> = BTreeMap::new();
    for (name, ty) in fields {
        let proto_field = ident::proto_field_name(name);
        let kind = resolve_prost_field_accessor_kind(resolve, ty);
        for accessor in ident::prost_field_accessors(&proto_field, kind) {
            if let Some(prev) = accessors.get(&accessor) {
                if !prev.eq_ignore_ascii_case(name) {
                    refuse_collision(
                        out,
                        location_for(name),
                        format!(
                            "{parent_label}: {member_kind}s `{prev}` and `{name}` both generate prost accessor `{accessor}`"
                        ),
                    );
                }
            } else {
                accessors.insert(accessor, name.to_string());
            }
        }
    }
}

fn resolve_wit_alias<'a>(resolve: &'a wit_parser::Resolve, mut ty: &'a Type) -> &'a Type {
    while let Type::Id(id) = ty {
        match &resolve.types[*id].kind {
            TypeDefKind::Type(inner) => ty = inner,
            _ => break,
        }
    }
    ty
}

fn is_wit_u8(resolve: &wit_parser::Resolve, ty: &Type) -> bool {
    matches!(resolve_wit_alias(resolve, ty), Type::U8)
}

fn is_wit_enum(resolve: &wit_parser::Resolve, ty: &Type) -> bool {
    matches!(
        resolve_wit_alias(resolve, ty),
        Type::Id(id) if matches!(resolve.types[*id].kind, TypeDefKind::Enum(_))
    )
}

fn is_wit_scalar_or_bytes(resolve: &wit_parser::Resolve, ty: &Type) -> bool {
    match resolve_wit_alias(resolve, ty) {
        Type::Bool
        | Type::U8
        | Type::U16
        | Type::U32
        | Type::U64
        | Type::S8
        | Type::S16
        | Type::S32
        | Type::S64
        | Type::F32
        | Type::F64
        | Type::String => true,
        Type::Id(id) => match &resolve.types[*id].kind {
            TypeDefKind::List(element) => is_wit_u8(resolve, element),
            _ => false,
        },
        Type::Char | Type::ErrorContext => false,
    }
}

fn resolve_prost_field_accessor_kind(
    resolve: &wit_parser::Resolve,
    ty: &Type,
) -> ident::ProstFieldAccessorKind {
    match resolve_wit_alias(resolve, ty) {
        Type::Bool
        | Type::U8
        | Type::U16
        | Type::U32
        | Type::U64
        | Type::S8
        | Type::S16
        | Type::S32
        | Type::S64
        | Type::F32
        | Type::F64
        | Type::String => ident::ProstFieldAccessorKind::OptionalScalar,
        Type::Id(id) => match &resolve.types[*id].kind {
            TypeDefKind::Enum(_) => ident::ProstFieldAccessorKind::OptionalEnum,
            TypeDefKind::List(element) => {
                if is_wit_u8(resolve, element) {
                    ident::ProstFieldAccessorKind::OptionalScalar
                } else if is_wit_enum(resolve, element) {
                    ident::ProstFieldAccessorKind::RepeatedEnum
                } else {
                    ident::ProstFieldAccessorKind::None
                }
            }
            TypeDefKind::Option(payload) => {
                if is_wit_enum(resolve, payload) {
                    ident::ProstFieldAccessorKind::OptionalEnum
                } else if is_wit_scalar_or_bytes(resolve, payload) {
                    ident::ProstFieldAccessorKind::OptionalScalar
                } else {
                    ident::ProstFieldAccessorKind::None
                }
            }
            _ => ident::ProstFieldAccessorKind::None,
        },
        Type::Char | Type::ErrorContext => ident::ProstFieldAccessorKind::None,
    }
}
