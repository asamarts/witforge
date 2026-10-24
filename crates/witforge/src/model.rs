//! The members of a contract that need stable wire numbers.

use wit_parser::TypeDefKind;

use crate::Contract;

/// One numbered member: a record field, a variant case, an enum case, or a flag.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Member {
    /// The owning type, as `namespace:package/interface/type`. Wire numbers are unique within it.
    pub parent: String,
    /// The member's name within its parent.
    pub name: String,
}

impl Member {
    /// The lockfile key, `namespace:package/interface/type.member`.
    ///
    /// The package version is deliberately not part of the key, so a version bump never
    /// renumbers anything.
    #[must_use]
    pub fn key(&self) -> String {
        format!("{}.{}", self.parent, self.name)
    }
}

/// Every numbered member of the root package's interface types, in a deterministic order.
#[must_use]
pub fn members(contract: &Contract) -> Vec<Member> {
    let resolve = &contract.resolve;
    let package = &resolve.packages[contract.package];
    let pkg = format!("{}:{}", package.name.namespace, package.name.name);
    let mut out = Vec::new();
    for (iface_name, &iface_id) in &package.interfaces {
        let iface = &resolve.interfaces[iface_id];
        for (type_name, &type_id) in &iface.types {
            let def = &resolve.types[type_id];
            let parent = format!("{pkg}/{iface_name}/{type_name}");
            let names: Vec<&str> = match &def.kind {
                TypeDefKind::Record(r) => r.fields.iter().map(|f| f.name.as_str()).collect(),
                TypeDefKind::Variant(v) => v.cases.iter().map(|c| c.name.as_str()).collect(),
                TypeDefKind::Enum(e) => e.cases.iter().map(|c| c.name.as_str()).collect(),
                TypeDefKind::Flags(f) => f.flags.iter().map(|f| f.name.as_str()).collect(),
                // Includes `use` aliases (`TypeDefKind::Type`), which `wit-parser` creates owned
                // by the interface that names them: members are numbered only where defined.
                _ => continue,
            };
            out.extend(names.into_iter().map(|name| Member {
                parent: parent.clone(),
                name: name.to_string(),
            }));
        }
    }
    out
}
