//! The restricted WIT subset that every contract must stay inside.
//!
//! Version 1 of the subset is pure and maps cleanly onto Rust and protobuf: no resources or
//! handles, no `future` or `stream`, no `map`, no fixed-length lists, no `char`, no tuples, and no
//! types defined directly in a world. Anything the checker does not recognise is refused rather
//! than passed through, so a new `wit-parser` construct cannot slip into the generated bindings.

use std::fmt;

use wit_parser::{Type, TypeDefKind, TypeId, TypeOwner};

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
        out.push(Violation {
            location: pkg.clone(),
            message: "the contract package needs a version (`package ns:name@x.y.z;`) so `@since` \
                      gates and evolution checks are possible"
                .into(),
        });
    }

    for (_, def) in &resolve.types {
        let (TypeOwner::World(world_id), Some(name)) = (def.owner, def.name.as_deref()) else {
            continue;
        };
        if resolve.worlds[world_id].package != Some(contract.package) {
            continue;
        }
        out.push(Violation {
            location: format!("{pkg}/{}/{name}", resolve.worlds[world_id].name),
            message: "types must be defined in an interface, not directly in a world".into(),
        });
    }

    for (iface_name, &iface_id) in &package.interfaces {
        let iface = &resolve.interfaces[iface_id];
        for (type_name, &type_id) in &iface.types {
            if resolve.types[type_id].owner != TypeOwner::Interface(iface_id) {
                continue;
            }
            let at = format!("{pkg}/{iface_name}/{type_name}");
            check_def(contract, type_id, &at, &mut out);
        }
        for (func_name, func) in &iface.functions {
            let at = format!("{pkg}/{iface_name}#{func_name}");
            for param in &func.params {
                check_type(
                    contract,
                    &param.ty,
                    &format!("{at}({})", param.name),
                    &mut out,
                );
            }
            if let Some(result) = &func.result {
                check_type(contract, result, &format!("{at} -> result"), &mut out);
            }
        }
    }
    out
}

fn check_def(contract: &Contract, id: TypeId, at: &str, out: &mut Vec<Violation>) {
    let refuse = |out: &mut Vec<Violation>, message: &str| {
        out.push(Violation {
            location: at.to_string(),
            message: message.to_string(),
        });
    };
    match &contract.resolve.types[id].kind {
        TypeDefKind::Record(r) => {
            if r.fields.is_empty() {
                refuse(out, "empty records are not supported; add a field or use a variant case without payload");
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
        TypeDefKind::Tuple(_) => refuse(out, "tuples are not supported; use a named record"),
        TypeDefKind::Resource | TypeDefKind::Handle(_) => refuse(
            out,
            "resources and handles are not supported; contract operations are pure functions over values",
        ),
        TypeDefKind::Map(..) => refuse(out, "`map` is not supported yet; use `list<record>`"),
        TypeDefKind::FixedLengthList(..) => refuse(out, "fixed-length lists are not supported; use `list`"),
        TypeDefKind::Future(_) | TypeDefKind::Stream(_) => {
            refuse(out, "`future` and `stream` are not supported; the contract is synchronous");
        }
        TypeDefKind::Unknown => refuse(out, "unresolved type"),
    }
}

fn check_type(contract: &Contract, ty: &Type, at: &str, out: &mut Vec<Violation>) {
    match ty {
        Type::Char => out.push(Violation {
            location: at.to_string(),
            message: "`char` is not supported; use `string`".into(),
        }),
        Type::ErrorContext => out.push(Violation {
            location: at.to_string(),
            message: "`error-context` is not supported".into(),
        }),
        // Anonymous types (`option<T>`, `list<T>`, `result<..>`, tuples) are checked where they
        // are used; named types are checked at their own definition.
        Type::Id(id) if contract.resolve.types[*id].name.is_none() => {
            check_def(contract, *id, at, out);
        }
        _ => {}
    }
}
