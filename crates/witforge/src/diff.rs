//! Contract diffing and breaking-change classification.
//!
//! A diff compares two version directories, or the highest version under two WIT roots. It
//! reports source changes together with three independent compatibility views:
//!
//! - **wire identity** is the append-only lockfile's numbers, kinds, and canonical types. A
//!   renumbering, kind change, or type change breaks it; an addition or a reserved removal does
//!   not.
//! - **source** is the WIT/component shape. Adding, removing, retyping, or reordering anything
//!   that participates in a component type breaks it, as does a world import or export change.
//! - **generated API** is the Rust surface: record fields, variant cases, enum cases, flags,
//!   functions, parameters, and named type shapes. Adding a field, case, flag, function, or
//!   parameter is API-breaking even when it is wire-compatible, because generated structs and
//!   traits are exhaustive. Reordering named fields is not API-breaking; reordering function
//!   parameters is.
//!
//! Added record fields whose type has a default (`option<T>`, `list<T>`, or an alias or named
//! definition of one) are marked `defaulted`: the generated old-to-new conversion can fill them,
//! but the source and generated API still change.
//!
//! Root-package docs and stability attributes are not compared. They do not affect wire identity,
//! generated code, or the component type. Dependency metadata is part of the exact pinned graph;
//! a graph mismatch is refused before local declarations are compared.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::emit::versions::ensure_same_dependency_graph;
use crate::lock::{PackageName, WireLock};
use crate::model::{
    Interface, Member, Model, TypeDef, TypeDefKind, TypeExpr, Version, WorldInterface,
};
use crate::{ContractError, ensure_directory_version, ensure_version_directory, load, subset};

/// One endpoint of a diff: the selected version directory and its identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    /// The version directory that was compared.
    pub path: PathBuf,
    /// The package version.
    pub version: Version,
    /// The package name as `namespace:name`.
    pub package: String,
}

/// The kind of one contract change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChangeKind {
    /// A declaration exists only in the new version.
    Added {
        /// Whether an added record field has a defaultable type.
        defaulted: bool,
    },
    /// A declaration exists only in the old version.
    Removed,
    /// Common members changed declaration order.
    Reordered {
        /// Their old relative order.
        before: Vec<String>,
        /// Their new relative order.
        after: Vec<String>,
    },
    /// A member or type kept its name but changed type.
    Retyped {
        /// The old type.
        from: String,
        /// The new type.
        to: String,
    },
    /// A named type kept its name but changed kind, such as record to enum.
    KindChanged {
        /// The old kind.
        from: String,
        /// The new kind.
        to: String,
    },
    /// A common member kept its kind and type but changed wire number.
    Renumbered {
        /// The old number.
        from: u32,
        /// The new number.
        to: u32,
    },
}

impl ChangeKind {
    fn rank(&self) -> u8 {
        match self {
            Self::KindChanged { .. } => 0,
            Self::Retyped { .. } => 1,
            Self::Renumbered { .. } => 2,
            Self::Removed => 3,
            Self::Added { .. } => 4,
            Self::Reordered { .. } => 5,
        }
    }
}

/// One change and its three compatibility verdicts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    /// What changed.
    pub kind: ChangeKind,
    /// The kind of declaration, such as `record field` or `function`.
    pub element: String,
    /// The WIT path, without the package name.
    pub path: String,
    /// The wire number when the change concerns a numbered member.
    pub number: Option<u32>,
    /// Whether the change breaks wire identity.
    pub wire_breaking: bool,
    /// Whether the change breaks the WIT/component source shape.
    pub source_breaking: bool,
    /// Whether the change breaks the generated Rust API.
    pub api_breaking: bool,
}

impl Change {
    /// Whether any compatibility view is broken.
    #[must_use]
    pub fn is_breaking(&self) -> bool {
        self.wire_breaking || self.source_breaking || self.api_breaking
    }
}

/// Counts over a [`DiffReport`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Summary {
    /// Total changes.
    pub changes: usize,
    /// Changes breaking at least one view.
    pub breaking: usize,
    /// Changes breaking wire identity.
    pub wire_breaking: usize,
    /// Changes breaking the WIT/component source shape.
    pub source_breaking: usize,
    /// Changes breaking the generated Rust API.
    pub api_breaking: usize,
}

/// A complete comparison of two contract versions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffReport {
    /// The old endpoint.
    pub old: Endpoint,
    /// The new endpoint.
    pub new: Endpoint,
    /// Changes in deterministic path order.
    pub changes: Vec<Change>,
    /// Counts over `changes`.
    pub summary: Summary,
}

/// Compare two version directories, or the highest version under two WIT roots.
///
/// A path containing `wire.lock` is a version directory. Any other path is a WIT root, and
/// the WIT-root loader selects its highest version. Both endpoints must declare the same
/// `namespace:name` and resolve the identical pinned dependency graph.
///
/// # Errors
///
/// Returns an error for an unreadable path, an invalid WIT package or lockfile, a version
/// directory that does not match its package version, a stale or unlocked live member, two
/// different package names, or mismatched dependency graphs. Contract violations carry
/// [`ContractError`].
pub fn compare(old: &Path, new: &Path) -> Result<DiffReport> {
    let old = load_endpoint(old)
        .with_context(|| format!("failed to read the old contract {}", old.display()))?;
    let new = load_endpoint(new)
        .with_context(|| format!("failed to read the new contract {}", new.display()))?;
    if old.endpoint.package != new.endpoint.package {
        bail!(ContractError::new(format!(
            "cannot diff package {} against {}: the package names differ",
            old.endpoint.package, new.endpoint.package
        )));
    }
    ensure_same_dependency_graph(&old.model.dependencies, &new.model.dependencies)?;
    let mut builder = Builder {
        old: &old,
        new: &new,
        changes: Vec::new(),
    };
    builder.compare_interfaces()?;
    builder.compare_worlds();
    let mut changes = builder.changes;
    changes.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.kind.rank().cmp(&right.kind.rank()))
            .then_with(|| left.element.cmp(&right.element))
    });
    let summary = summarize(&changes);
    Ok(DiffReport {
        old: old.endpoint,
        new: new.endpoint,
        changes,
        summary,
    })
}

fn summarize(changes: &[Change]) -> Summary {
    Summary {
        changes: changes.len(),
        breaking: changes.iter().filter(|change| change.is_breaking()).count(),
        wire_breaking: changes.iter().filter(|change| change.wire_breaking).count(),
        source_breaking: changes
            .iter()
            .filter(|change| change.source_breaking)
            .count(),
        api_breaking: changes.iter().filter(|change| change.api_breaking).count(),
    }
}

struct Loaded {
    endpoint: Endpoint,
    model: Model,
    members: BTreeMap<String, Member>,
    numbers: BTreeMap<String, u32>,
    entries: BTreeMap<String, (u32, bool)>,
}

impl Loaded {
    fn new(path: &Path, model: Model, lock: &WireLock) -> Result<Self> {
        let package = format!("{}:{}", model.package.namespace, model.package.name);
        let package_name = PackageName {
            namespace: model.package.namespace.clone(),
            name: model.package.name.clone(),
        };
        lock.check_package(&package_name)
            .map_err(|error| ContractError::from_error(&error))?;
        let members: BTreeMap<String, Member> = model
            .members()?
            .into_iter()
            .map(|member| (member.key(), member))
            .collect();
        let entries: BTreeMap<String, (u32, bool)> = lock
            .entries()
            .map(|(key, entry)| (key.key(), (entry.number, entry.reserved)))
            .collect();
        let numbers: BTreeMap<String, u32> = entries
            .iter()
            .map(|(key, (number, _))| (key.clone(), *number))
            .collect();
        for key in members.keys() {
            let Some((_, reserved)) = entries.get(key) else {
                bail!(ContractError::new(format!(
                    "live member `{key}` has no wire number; run `witforge lock`"
                )));
            };
            if *reserved {
                bail!(ContractError::new(format!(
                    "live member `{key}` is reserved in the lockfile; run `witforge lock`"
                )));
            }
        }
        Ok(Self {
            endpoint: Endpoint {
                path: path.to_path_buf(),
                version: model.package.version,
                package,
            },
            model,
            members,
            numbers,
            entries,
        })
    }

    fn number(&self, parent: &str, name: &str) -> Result<u32> {
        let key = format!("{}/{parent}.{name}", self.endpoint.package);
        self.numbers.get(&key).copied().with_context(|| {
            format!(
                "member `{key}` has no wire number in {}",
                self.endpoint.path.display()
            )
        })
    }

    fn preserves(&self, old: &Self, key: &str) -> bool {
        let Some((old_number, _)) = old.entries.get(key) else {
            return true;
        };
        matches!(self.entries.get(key), Some((number, true)) if number == old_number)
    }

    fn preserves_scope(&self, old: &Self, prefix: &str) -> bool {
        old.entries
            .iter()
            .filter(|(key, _)| {
                key.starts_with(&format!("{prefix}/"))
                    || key.starts_with(&format!("{prefix}."))
                    || key.starts_with(&format!("{prefix}#"))
            })
            .all(|(key, _)| self.preserves(old, key))
    }

    fn member(&self, parent: &str, name: &str) -> Result<&Member> {
        let key = format!("{}/{parent}.{name}", self.endpoint.package);
        self.members.get(&key).with_context(|| {
            format!(
                "member `{key}` is missing from the model of {}",
                self.endpoint.path.display()
            )
        })
    }

    fn member_type(&self, parent: &str, name: &str) -> Result<String> {
        Ok(self
            .member(parent, name)?
            .ty
            .clone()
            .unwrap_or_else(|| "_".to_string()))
    }
}

fn load_endpoint(path: &Path) -> Result<Loaded> {
    if path.join("wire.lock").is_file() {
        return load_version(path);
    }
    let mut versions = crate::emit::versions::load_versions(path)?;
    let version = versions.pop().with_context(|| {
        format!(
            "no version directories with a wire.lock under {}",
            path.display()
        )
    })?;
    Loaded::new(&version.directory, version.model, &version.lock)
}

fn load_version(path: &Path) -> Result<Loaded> {
    ensure_version_directory(path)?;
    let contract = load(path)?;
    let violations = subset::check(&contract);
    if !violations.is_empty() {
        let messages: Vec<String> = violations.iter().map(ToString::to_string).collect();
        bail!(ContractError::new(format!(
            "{}: {}",
            path.display(),
            messages.join("; ")
        )));
    }
    let model = crate::model::build(&contract)?;
    ensure_directory_version(path, &model)?;
    let lock_path = path.join("wire.lock");
    let text = fs::read_to_string(&lock_path)
        .with_context(|| format!("failed to read {}", lock_path.display()))?;
    let lock = WireLock::parse(&text).with_context(|| format!("in {}", lock_path.display()))?;
    Loaded::new(path, model, &lock)
}

struct Builder<'a> {
    old: &'a Loaded,
    new: &'a Loaded,
    changes: Vec<Change>,
}

#[derive(Clone, Copy, Default)]
struct Breaking {
    wire: bool,
    source: bool,
    api: bool,
}

impl Breaking {
    const NONE: Self = Self {
        wire: false,
        source: false,
        api: false,
    };
    const ALL: Self = Self {
        wire: true,
        source: true,
        api: true,
    };
    const SOURCE_API: Self = Self {
        wire: false,
        source: true,
        api: true,
    };
    const SOURCE: Self = Self {
        wire: false,
        source: true,
        api: false,
    };
    const WIRE: Self = Self {
        wire: true,
        source: false,
        api: false,
    };
}

struct MemberComparison<'a> {
    element: &'a str,
    path: &'a str,
    parent: &'a str,
    api_on_reorder: bool,
    defaulted: &'a BTreeSet<String>,
}

impl Builder<'_> {
    fn push(
        &mut self,
        kind: ChangeKind,
        element: impl Into<String>,
        path: impl Into<String>,
        number: Option<u32>,
        breaking: Breaking,
    ) {
        self.changes.push(Change {
            kind,
            element: element.into(),
            path: path.into(),
            number,
            wire_breaking: breaking.wire,
            source_breaking: breaking.source,
            api_breaking: breaking.api,
        });
    }

    fn compare_interfaces(&mut self) -> Result<()> {
        let old: BTreeMap<&str, &Interface> = self
            .old
            .model
            .interfaces
            .iter()
            .map(|interface| (interface.name.as_str(), interface))
            .collect();
        let new: BTreeMap<&str, &Interface> = self
            .new
            .model
            .interfaces
            .iter()
            .map(|interface| (interface.name.as_str(), interface))
            .collect();
        for name in new.keys().filter(|name| !old.contains_key(*name)) {
            self.push(
                ChangeKind::Added { defaulted: false },
                "interface",
                *name,
                None,
                Breaking::NONE,
            );
        }
        for name in old.keys().filter(|name| !new.contains_key(*name)) {
            let prefix = format!("{}/{name}", self.old.endpoint.package);
            self.push(
                ChangeKind::Removed,
                "interface",
                *name,
                None,
                Breaking {
                    wire: !self.new.preserves_scope(self.old, &prefix),
                    source: true,
                    api: true,
                },
            );
        }
        for (name, old_interface) in &old {
            let Some(new_interface) = new.get(name) else {
                continue;
            };
            self.compare_types(old_interface, new_interface)?;
            self.compare_functions(old_interface, new_interface)?;
        }
        Ok(())
    }

    fn compare_types(&mut self, old: &Interface, new: &Interface) -> Result<()> {
        let old_types: BTreeMap<&str, &TypeDef> = old
            .types
            .iter()
            .map(|def| (def.name.as_str(), def))
            .collect();
        let new_types: BTreeMap<&str, &TypeDef> = new
            .types
            .iter()
            .map(|def| (def.name.as_str(), def))
            .collect();
        for name in new_types
            .keys()
            .filter(|name| !old_types.contains_key(*name))
        {
            self.push(
                ChangeKind::Added { defaulted: false },
                "type",
                format!("{}/{}", old.name, name),
                None,
                Breaking::NONE,
            );
        }
        for name in old_types
            .keys()
            .filter(|name| !new_types.contains_key(*name))
        {
            let prefix = format!("{}/{}/{name}", self.old.endpoint.package, old.name);
            self.push(
                ChangeKind::Removed,
                "type",
                format!("{}/{}", old.name, name),
                None,
                Breaking {
                    wire: !self.new.preserves_scope(self.old, &prefix),
                    source: true,
                    api: true,
                },
            );
        }
        for (name, old_def) in &old_types {
            let Some(new_def) = new_types.get(name) else {
                continue;
            };
            self.compare_type(old, old_def, new_def)?;
        }
        Ok(())
    }

    fn compare_type(&mut self, interface: &Interface, old: &TypeDef, new: &TypeDef) -> Result<()> {
        let path = format!("{}/{}", interface.name, old.name);
        let parent = format!("{}/{}", interface.name, old.name);
        let old_kind = type_kind_name(&old.kind);
        let new_kind = type_kind_name(&new.kind);
        if old_kind != new_kind {
            let prefix = format!(
                "{}/{}/{}",
                self.old.endpoint.package, interface.name, old.name
            );
            self.push(
                ChangeKind::KindChanged {
                    from: old_kind.to_string(),
                    to: new_kind.to_string(),
                },
                "type",
                path,
                None,
                Breaking {
                    wire: !self.new.preserves_scope(self.old, &prefix),
                    source: true,
                    api: true,
                },
            );
            return Ok(());
        }
        match (&old.kind, &new.kind) {
            (TypeDefKind::Record(old_fields), TypeDefKind::Record(new_fields)) => {
                self.compare_record(interface, old, old_fields, new_fields)
            }
            (TypeDefKind::Variant(old_cases), TypeDefKind::Variant(new_cases)) => {
                self.compare_cases(interface, old, old_cases, new_cases, "variant case")
            }
            (TypeDefKind::Enum(old_cases), TypeDefKind::Enum(new_cases)) => {
                self.compare_cases(interface, old, old_cases, new_cases, "enum case")
            }
            (TypeDefKind::Flags(old_flags), TypeDefKind::Flags(new_flags)) => {
                let old_names: Vec<String> =
                    old_flags.iter().map(|flag| flag.name.clone()).collect();
                let new_names: Vec<String> =
                    new_flags.iter().map(|flag| flag.name.clone()).collect();
                self.compare_members(
                    &member_pairs(self.old, &parent, &old_names)?,
                    &member_pairs(self.new, &parent, &new_names)?,
                    &MemberComparison {
                        element: "flag",
                        path: &path,
                        parent: &parent,
                        api_on_reorder: false,
                        defaulted: &BTreeSet::new(),
                    },
                )
            }
            _ => {
                let old_shape = type_shape(&old.kind);
                let new_shape = type_shape(&new.kind);
                if old_shape != new_shape {
                    self.push(
                        ChangeKind::Retyped {
                            from: old_shape,
                            to: new_shape,
                        },
                        "type",
                        path,
                        None,
                        Breaking::SOURCE_API,
                    );
                }
                Ok(())
            }
        }
    }

    fn compare_record(
        &mut self,
        interface: &Interface,
        old: &TypeDef,
        old_fields: &[crate::model::Field],
        new_fields: &[crate::model::Field],
    ) -> Result<()> {
        let path = format!("{}/{}", interface.name, old.name);
        let parent = format!("{}/{}", interface.name, old.name);
        let old_names: Vec<String> = old_fields.iter().map(|field| field.name.clone()).collect();
        let new_names: Vec<String> = new_fields.iter().map(|field| field.name.clone()).collect();
        let defaulted: BTreeSet<String> = new_fields
            .iter()
            .filter(|field| defaultable(&self.new.model, &field.ty))
            .map(|field| field.name.clone())
            .collect();
        self.compare_members(
            &member_pairs(self.old, &parent, &old_names)?,
            &member_pairs(self.new, &parent, &new_names)?,
            &MemberComparison {
                element: "record field",
                path: &path,
                parent: &parent,
                api_on_reorder: false,
                defaulted: &defaulted,
            },
        )
    }

    fn compare_cases(
        &mut self,
        interface: &Interface,
        old: &TypeDef,
        old_cases: &[crate::model::Case],
        new_cases: &[crate::model::Case],
        element: &str,
    ) -> Result<()> {
        let path = format!("{}/{}", interface.name, old.name);
        let parent = format!("{}/{}", interface.name, old.name);
        let old_names: Vec<String> = old_cases.iter().map(|case| case.name.clone()).collect();
        let new_names: Vec<String> = new_cases.iter().map(|case| case.name.clone()).collect();
        self.compare_members(
            &member_pairs(self.old, &parent, &old_names)?,
            &member_pairs(self.new, &parent, &new_names)?,
            &MemberComparison {
                element,
                path: &path,
                parent: &parent,
                api_on_reorder: false,
                defaulted: &BTreeSet::new(),
            },
        )
    }

    fn compare_members(
        &mut self,
        old: &[(String, String)],
        new: &[(String, String)],
        spec: &MemberComparison<'_>,
    ) -> Result<()> {
        let MemberComparison {
            element,
            path,
            parent,
            api_on_reorder,
            defaulted,
        } = spec;
        let old_map: BTreeMap<&str, &str> = old
            .iter()
            .map(|(name, ty)| (name.as_str(), ty.as_str()))
            .collect();
        let new_map: BTreeMap<&str, &str> = new
            .iter()
            .map(|(name, ty)| (name.as_str(), ty.as_str()))
            .collect();
        for name in new_map.keys().filter(|name| !old_map.contains_key(**name)) {
            self.push(
                ChangeKind::Added {
                    defaulted: defaulted.contains(*name),
                },
                *element,
                format!("{path}.{name}"),
                Some(self.new.number(parent, name)?),
                Breaking::SOURCE_API,
            );
        }
        for name in old_map.keys().filter(|name| !new_map.contains_key(**name)) {
            let number = self.old.number(parent, name)?;
            let key = format!("{}/{parent}.{name}", self.old.endpoint.package);
            self.push(
                ChangeKind::Removed,
                *element,
                format!("{path}.{name}"),
                Some(number),
                Breaking {
                    wire: !self.new.preserves(self.old, &key),
                    source: true,
                    api: true,
                },
            );
        }
        for (name, old_ty) in &old_map {
            let Some(new_ty) = new_map.get(name) else {
                continue;
            };
            let old_number = self.old.number(parent, name)?;
            if old_ty != new_ty {
                self.push(
                    ChangeKind::Retyped {
                        from: (*old_ty).to_string(),
                        to: (*new_ty).to_string(),
                    },
                    *element,
                    format!("{path}.{name}"),
                    Some(self.new.number(parent, name)?),
                    Breaking::ALL,
                );
            }
            let new_number = self.new.number(parent, name)?;
            if old_number != new_number {
                self.push(
                    ChangeKind::Renumbered {
                        from: old_number,
                        to: new_number,
                    },
                    *element,
                    format!("{path}.{name}"),
                    Some(new_number),
                    Breaking::WIRE,
                );
            }
        }
        let before: Vec<String> = old
            .iter()
            .filter(|(name, _)| new_map.contains_key(name.as_str()))
            .map(|(name, _)| name.clone())
            .collect();
        let after: Vec<String> = new
            .iter()
            .filter(|(name, _)| old_map.contains_key(name.as_str()))
            .map(|(name, _)| name.clone())
            .collect();
        if before != after {
            self.push(
                ChangeKind::Reordered { before, after },
                format!("{element} order"),
                *path,
                None,
                Breaking {
                    wire: false,
                    source: true,
                    api: *api_on_reorder,
                },
            );
        }
        Ok(())
    }

    fn compare_function(
        &mut self,
        interface: &str,
        name: &str,
        old: &crate::model::Function,
        new: &crate::model::Function,
    ) -> Result<()> {
        let path = format!("{interface}.{name}");
        let parent = format!("{interface}#{name}");
        let old_params: Vec<(String, String)> = old
            .params
            .iter()
            .map(|param| {
                Ok((
                    param.name.clone(),
                    self.old.member_type(&parent, &param.name)?,
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        let new_params: Vec<(String, String)> = new
            .params
            .iter()
            .map(|param| {
                Ok((
                    param.name.clone(),
                    self.new.member_type(&parent, &param.name)?,
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        self.compare_members(
            &old_params,
            &new_params,
            &MemberComparison {
                element: "parameter",
                path: &path,
                parent: &parent,
                api_on_reorder: true,
                defaulted: &BTreeSet::new(),
            },
        )?;
        let old_result = self.old.member_type(interface, name)?;
        let new_result = self.new.member_type(interface, name)?;
        if old_result != new_result {
            self.push(
                ChangeKind::Retyped {
                    from: old_result,
                    to: new_result,
                },
                "function result",
                path,
                Some(self.new.number(interface, name)?),
                Breaking::ALL,
            );
        }
        Ok(())
    }

    fn compare_functions(&mut self, old: &Interface, new: &Interface) -> Result<()> {
        let old_map: BTreeMap<&str, &crate::model::Function> = old
            .functions
            .iter()
            .map(|function| (function.name.as_str(), function))
            .collect();
        let new_map: BTreeMap<&str, &crate::model::Function> = new
            .functions
            .iter()
            .map(|function| (function.name.as_str(), function))
            .collect();
        for name in new_map.keys().filter(|name| !old_map.contains_key(*name)) {
            self.push(
                ChangeKind::Added { defaulted: false },
                "function",
                format!("{}.{}", old.name, name),
                Some(self.new.number(&old.name, name)?),
                Breaking::SOURCE_API,
            );
        }
        for name in old_map.keys().filter(|name| !new_map.contains_key(*name)) {
            let method = format!("{}/{}.{name}", self.old.endpoint.package, old.name);
            let params = format!("{}/{}#{name}", self.old.endpoint.package, old.name);
            self.push(
                ChangeKind::Removed,
                "function",
                format!("{}.{}", old.name, name),
                Some(self.old.number(&old.name, name)?),
                Breaking {
                    wire: !self.new.preserves(self.old, &method)
                        || !self.new.preserves_scope(self.old, &params),
                    source: true,
                    api: true,
                },
            );
        }
        for (name, old_function) in &old_map {
            let Some(new_function) = new_map.get(name) else {
                continue;
            };
            self.compare_function(&old.name, name, old_function, new_function)?;
        }
        let before: Vec<String> = old
            .functions
            .iter()
            .filter(|function| new_map.contains_key(function.name.as_str()))
            .map(|function| function.name.clone())
            .collect();
        let after: Vec<String> = new
            .functions
            .iter()
            .filter(|function| old_map.contains_key(function.name.as_str()))
            .map(|function| function.name.clone())
            .collect();
        if before != after {
            self.push(
                ChangeKind::Reordered { before, after },
                "function order",
                old.name.clone(),
                None,
                Breaking::SOURCE,
            );
        }
        Ok(())
    }

    fn compare_worlds(&mut self) {
        let old: BTreeMap<&str, &crate::model::World> = self
            .old
            .model
            .worlds
            .iter()
            .map(|world| (world.name.as_str(), world))
            .collect();
        let new: BTreeMap<&str, &crate::model::World> = self
            .new
            .model
            .worlds
            .iter()
            .map(|world| (world.name.as_str(), world))
            .collect();
        for name in new.keys().filter(|name| !old.contains_key(*name)) {
            self.push(
                ChangeKind::Added { defaulted: false },
                "world",
                *name,
                None,
                Breaking::NONE,
            );
        }
        for name in old.keys().filter(|name| !new.contains_key(*name)) {
            self.push(ChangeKind::Removed, "world", *name, None, Breaking::SOURCE);
        }
        for (name, old_world) in &old {
            let Some(new_world) = new.get(name) else {
                continue;
            };
            self.compare_world_side(name, "import", &old_world.imports, &new_world.imports);
            self.compare_world_side(name, "export", &old_world.exports, &new_world.exports);
        }
    }

    fn compare_world_side(
        &mut self,
        world: &str,
        side: &str,
        old: &[WorldInterface],
        new: &[WorldInterface],
    ) {
        let old_names: BTreeSet<String> = old.iter().map(WorldInterface::qualified_name).collect();
        let new_names: BTreeSet<String> = new.iter().map(WorldInterface::qualified_name).collect();
        for interface in new
            .iter()
            .filter(|i| !old_names.contains(&i.qualified_name()))
        {
            self.push(
                ChangeKind::Added { defaulted: false },
                format!("world {side}"),
                format!("{world}/{side}/{}", interface.qualified_name()),
                None,
                Breaking::SOURCE,
            );
        }
        for interface in old
            .iter()
            .filter(|i| !new_names.contains(&i.qualified_name()))
        {
            self.push(
                ChangeKind::Removed,
                format!("world {side}"),
                format!("{world}/{side}/{}", interface.qualified_name()),
                None,
                Breaking::SOURCE,
            );
        }
        let before: Vec<String> = old
            .iter()
            .filter(|interface| new_names.contains(&interface.qualified_name()))
            .map(WorldInterface::qualified_name)
            .collect();
        let after: Vec<String> = new
            .iter()
            .filter(|interface| old_names.contains(&interface.qualified_name()))
            .map(WorldInterface::qualified_name)
            .collect();
        if before != after {
            self.push(
                ChangeKind::Reordered { before, after },
                format!("world {side} order"),
                format!("{world}/{side}"),
                None,
                Breaking::SOURCE,
            );
        }
    }
}

fn member_pairs(loaded: &Loaded, parent: &str, names: &[String]) -> Result<Vec<(String, String)>> {
    names
        .iter()
        .map(|name| Ok((name.clone(), loaded.member_type(parent, name)?)))
        .collect()
}

fn type_kind_name(kind: &TypeDefKind) -> &'static str {
    match kind {
        TypeDefKind::Record(_) => "record",
        TypeDefKind::Variant(_) => "variant",
        TypeDefKind::Enum(_) => "enum",
        TypeDefKind::Flags(_) => "flags",
        TypeDefKind::Alias(_) => "alias",
        TypeDefKind::List(_) => "list",
        TypeDefKind::Option(_) => "option",
        TypeDefKind::Result(_) => "result",
    }
}

fn type_shape(kind: &TypeDefKind) -> String {
    match kind {
        TypeDefKind::Alias(target) => source_type(target),
        TypeDefKind::List(element) => format!("list<{}>", source_type(element)),
        TypeDefKind::Option(payload) => format!("option<{}>", source_type(payload)),
        TypeDefKind::Result(result) => format!(
            "result<{},{}>",
            result
                .ok
                .as_deref()
                .map_or_else(|| "_".to_string(), source_type),
            result
                .err
                .as_deref()
                .map_or_else(|| "_".to_string(), source_type),
        ),
        TypeDefKind::Record(_)
        | TypeDefKind::Variant(_)
        | TypeDefKind::Enum(_)
        | TypeDefKind::Flags(_) => type_kind_name(kind).to_string(),
    }
}

fn source_type(ty: &TypeExpr) -> String {
    match ty {
        TypeExpr::Bool => "bool".to_string(),
        TypeExpr::U8 => "u8".to_string(),
        TypeExpr::U16 => "u16".to_string(),
        TypeExpr::U32 => "u32".to_string(),
        TypeExpr::U64 => "u64".to_string(),
        TypeExpr::S8 => "s8".to_string(),
        TypeExpr::S16 => "s16".to_string(),
        TypeExpr::S32 => "s32".to_string(),
        TypeExpr::S64 => "s64".to_string(),
        TypeExpr::F32 => "f32".to_string(),
        TypeExpr::F64 => "f64".to_string(),
        TypeExpr::String => "string".to_string(),
        TypeExpr::Named(name) => name.to_string(),
        TypeExpr::List(element) => format!("list<{}>", source_type(element)),
        TypeExpr::Option(payload) => format!("option<{}>", source_type(payload)),
        TypeExpr::Result(result) => format!(
            "result<{},{}>",
            result
                .ok
                .as_deref()
                .map_or_else(|| "_".to_string(), source_type),
            result
                .err
                .as_deref()
                .map_or_else(|| "_".to_string(), source_type),
        ),
    }
}

fn defaultable(model: &Model, ty: &TypeExpr) -> bool {
    defaultable_inner(model, ty, &mut HashSet::new())
}

fn defaultable_inner(model: &Model, ty: &TypeExpr, visiting: &mut HashSet<String>) -> bool {
    match ty {
        TypeExpr::Option(_) | TypeExpr::List(_) => true,
        TypeExpr::Named(name) => {
            let key = name.to_string();
            if !visiting.insert(key.clone()) {
                return false;
            }
            let found = model.type_def(name).is_some_and(|def| match &def.kind {
                TypeDefKind::Option(_) | TypeDefKind::List(_) => true,
                TypeDefKind::Alias(target) => defaultable_inner(model, target, visiting),
                _ => false,
            });
            visiting.remove(&key);
            found
        }
        TypeExpr::Bool
        | TypeExpr::U8
        | TypeExpr::U16
        | TypeExpr::U32
        | TypeExpr::U64
        | TypeExpr::S8
        | TypeExpr::S16
        | TypeExpr::S32
        | TypeExpr::S64
        | TypeExpr::F32
        | TypeExpr::F64
        | TypeExpr::String
        | TypeExpr::Result(_) => false,
    }
}
