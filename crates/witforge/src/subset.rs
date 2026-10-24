//! The restricted WIT subset that every contract must stay inside.
//!
//! Version 1 of the subset is pure, synchronous, and self-contained, and maps cleanly onto Rust
//! and protobuf: no resources or handles, no `future`, `stream`, or `error-context`, no `map`, no
//! fixed-length lists, no `char`, no tuples, no `async` functions or `get`/`set` accessors, and
//! nothing from another package. Types and functions live in the package's named interfaces, and
//! worlds only import and export those interfaces. Anything the checker does not recognise is
//! refused rather than passed through, so a new `wit-parser` construct cannot slip into the
//! generated bindings.

use std::fmt;

use wit_parser::{
    Function, FunctionKind, PackageId, Type, TypeDefKind, TypeId, TypeOwner, WorldId, WorldItem,
    WorldKey,
};

use crate::Contract;

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

/// Check the root package of `contract` against the subset.
#[must_use]
pub fn check(contract: &Contract) -> Vec<Violation> {
    let resolve = &contract.resolve;
    let package = &resolve.packages[contract.package];
    let pkg = format!("{}:{}", package.name.namespace, package.name.name);
    let mut out = Vec::new();

    if package.name.version.is_none() {
        refuse(
            &mut out,
            &pkg,
            "the contract package needs a version (`package ns:name@x.y.z;`) so `@since` gates \
             and evolution checks are possible",
        );
    }

    for (iface_name, &iface_id) in &package.interfaces {
        let iface = &resolve.interfaces[iface_id];
        // `use` aliases are owned by the interface that names them, so they are checked here
        // too: an alias of a type from another package is refused at the `use`.
        for (type_name, &type_id) in &iface.types {
            let at = format!("{pkg}/{iface_name}/{type_name}");
            check_def(contract, type_id, &at, &mut out);
        }
        for (func_name, func) in &iface.functions {
            let at = format!("{pkg}/{iface_name}#{func_name}");
            check_function(contract, func, &at, &mut out);
        }
    }

    for (world_name, &world_id) in &package.worlds {
        check_world(contract, world_id, &format!("{pkg}/{world_name}"), &mut out);
    }
    out
}

fn check_world(contract: &Contract, world_id: WorldId, at: &str, out: &mut Vec<Violation>) {
    let resolve = &contract.resolve;
    let world = &resolve.worlds[world_id];
    for (verb, items) in [("imports", &world.imports), ("exports", &world.exports)] {
        for (key, item) in items {
            match (key, item) {
                (WorldKey::Name(name), WorldItem::Interface { .. }) => refuse(
                    out,
                    format!("{at}/{name}"),
                    "inline interfaces are not supported; define a named interface in the \
                     package and import or export it",
                ),
                (WorldKey::Interface(_), WorldItem::Interface { id, .. }) => {
                    let iface = &resolve.interfaces[*id];
                    if iface.package != Some(contract.package) {
                        // `id_of` panics on an interface without a package.
                        let name = iface
                            .package
                            .and_then(|_| resolve.id_of(*id))
                            .unwrap_or_default();
                        refuse(
                            out,
                            at,
                            format!(
                                "{verb} `{name}` from another package; a contract must be \
                                 self-contained"
                            ),
                        );
                    }
                }
                (_, WorldItem::Function(func)) => refuse(
                    out,
                    format!("{at}#{}", func.name),
                    "functions must be defined in an interface; export the interface from the \
                     world",
                ),
                (_, WorldItem::Type { id, .. }) => {
                    check_world_type(contract, world_id, *id, at, out);
                }
            }
        }
    }
}

fn check_world_type(
    contract: &Contract,
    world_id: WorldId,
    id: TypeId,
    at: &str,
    out: &mut Vec<Violation>,
) {
    let resolve = &contract.resolve;
    let def = &resolve.types[id];
    let at = format!("{at}/{}", def.name.as_deref().unwrap_or_default());
    match &def.kind {
        // Brought in by `include`: checked where it is defined, or refused if that is in
        // another package.
        _ if def.owner != TypeOwner::World(world_id) => {
            check_type(contract, &Type::Id(id), &at, out);
        }
        // `use` in a world names a type defined in an interface; it defines nothing itself.
        TypeDefKind::Type(target @ Type::Id(target_id))
            if resolve.types[*target_id].name.is_some() =>
        {
            check_type(contract, target, &at, out);
        }
        _ => refuse(
            out,
            at,
            "types must be defined in an interface, not directly in a world",
        ),
    }
}

fn check_function(contract: &Contract, func: &Function, at: &str, out: &mut Vec<Violation>) {
    match func.kind {
        FunctionKind::Freestanding => {
            for param in &func.params {
                check_type(contract, &param.ty, &format!("{at}({})", param.name), out);
            }
            if let Some(result) = &func.result {
                check_type(contract, result, &format!("{at} -> result"), out);
            }
        }
        FunctionKind::AsyncFreestanding => refuse(
            out,
            at,
            "`async` functions are not supported; the contract is synchronous",
        ),
        FunctionKind::Getter | FunctionKind::Setter => refuse(
            out,
            at,
            "`get` and `set` accessors are not supported; use a function",
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

fn check_def(contract: &Contract, id: TypeId, at: &str, out: &mut Vec<Violation>) {
    match &contract.resolve.types[id].kind {
        TypeDefKind::Record(r) => {
            // `wit-parser` already rejects empty records; kept in case a bump stops doing so.
            if r.fields.is_empty() {
                refuse(out, at, "empty records are not supported; add a field or use a variant case without payload");
            }
            for field in &r.fields {
                check_type(contract, &field.ty, &format!("{at}.{}", field.name), out);
            }
        }
        TypeDefKind::Variant(v) => {
            for case in &v.cases {
                if let Some(ty) = &case.ty {
                    check_type(contract, ty, &format!("{at}.{}", case.name), out);
                }
            }
        }
        TypeDefKind::Enum(_) | TypeDefKind::Flags(_) => {}
        TypeDefKind::Option(ty) | TypeDefKind::List(ty) | TypeDefKind::Type(ty) => {
            check_type(contract, ty, at, out);
        }
        TypeDefKind::Result(r) => {
            for ty in [&r.ok, &r.err].into_iter().flatten() {
                check_type(contract, ty, at, out);
            }
        }
        TypeDefKind::Tuple(_) => refuse(out, at, "tuples are not supported; use a named record"),
        TypeDefKind::Resource | TypeDefKind::Handle(_) => refuse(
            out,
            at,
            "resources and handles are not supported; contract operations are pure functions over values",
        ),
        TypeDefKind::Map(..) => refuse(out, at, "`map` is not supported yet; use `list<record>`"),
        TypeDefKind::FixedLengthList(..) => refuse(out, at, "fixed-length lists are not supported; use `list`"),
        TypeDefKind::Future(_) | TypeDefKind::Stream(_) => {
            refuse(out, at, "`future` and `stream` are not supported; the contract is synchronous");
        }
        TypeDefKind::Unknown => refuse(out, at, "unresolved type"),
    }
}

fn check_type(contract: &Contract, ty: &Type, at: &str, out: &mut Vec<Violation>) {
    match ty {
        Type::Char => refuse(out, at, "`char` is not supported; use `string`"),
        Type::ErrorContext => refuse(out, at, "`error-context` is not supported"),
        Type::Id(id) => {
            let def = &contract.resolve.types[*id];
            match &def.name {
                // Anonymous types (`option<T>`, `list<T>`, `result<..>`, tuples) are checked
                // where they are used.
                None => check_def(contract, *id, at, out),
                // Named types are checked at their own definition, which must be in this
                // package: the lockfile and the emitters cover only the contract's package.
                Some(name) => {
                    let package = package_of(contract, def.owner);
                    if package != Some(contract.package) {
                        let from = package.map_or_else(String::new, |p| {
                            contract.resolve.packages[p].name.to_string()
                        });
                        refuse(
                            out,
                            at,
                            format!(
                                "uses `{name}` from another package (`{from}`); a contract must \
                                 be self-contained"
                            ),
                        );
                    }
                }
            }
        }
        _ => {}
    }
}

fn package_of(contract: &Contract, owner: TypeOwner) -> Option<PackageId> {
    match owner {
        TypeOwner::Interface(id) => contract.resolve.interfaces[id].package,
        TypeOwner::World(id) => contract.resolve.worlds[id].package,
        TypeOwner::None => None,
    }
}

fn refuse(out: &mut Vec<Violation>, location: impl Into<String>, message: impl Into<String>) {
    out.push(Violation {
        location: location.into(),
        message: message.into(),
    });
}
