//! The owned internal model of one contract version.
//!
//! The model is the boundary between `wit-parser` and the rest of witforge. It contains no
//! `wit-parser` types, so an upstream bump only changes the conversion code in [`build`]. The
//! subset checker runs before construction; the conversion also refuses anything it cannot
//! represent, such as resources, tuples, and world-defined types. Named types from standard
//! `deps/` packages are represented with their package identity.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fmt;
use std::path::PathBuf;

use anyhow::{Context, Result, bail, ensure};
use wit_parser::{
    Docs as WitDocs, Function as WitFunction, FunctionKind, InterfaceId, PackageId, PackageName,
    Resolve, Span, Stability as WitStability, Type, TypeDefKind as WitTypeDefKind, TypeId,
    TypeOwner, WorldId, WorldItem, WorldKey,
};

use crate::Contract;

/// Maximum nested `list`, `option`, and `result` levels in one type expression.
pub(crate) const MAX_TYPE_NESTING: usize = 32;
/// Maximum number of alias links followed while resolving one type expression.
pub(crate) const MAX_ALIAS_CHAIN: usize = 32;
/// Maximum bytes in one canonical type string.
pub(crate) const MAX_CANONICAL_TYPE_BYTES: usize = 1024;
/// Maximum bytes in one synthesized protobuf wrapper name.
pub(crate) const MAX_WRAPPER_NAME_BYTES: usize = 1024;

/// A contract version, used for package versions and `@since` values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub struct Version {
    /// Major version.
    pub major: u64,
    /// Minor version.
    pub minor: u64,
    /// Patch version.
    pub patch: u64,
}

impl Version {
    /// Create a contract version.
    #[must_use]
    pub const fn new(major: u64, minor: u64, patch: u64) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// WIT stability and deprecation metadata attached to an item.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Stability {
    /// No `@since`, `@unstable`, or `@deprecated` attribute.
    Unknown,
    /// Explicitly tagged `@unstable(feature = ...)`.
    Unstable {
        /// The feature name.
        feature: String,
    },
    /// Explicitly tagged `@since(version = ...)`.
    Stable {
        /// The version since which the item is stable.
        since: Version,
    },
    /// Explicitly tagged `@deprecated(version = ...)` beside `@since` or `@unstable`.
    Deprecated {
        /// The stable version that introduced the item, when it has `@since`.
        since: Option<Version>,
        /// The unstable feature that owns the item, when it has `@unstable`.
        feature: Option<String>,
        /// The version that deprecated the item.
        deprecated: Version,
        /// The deterministic note emitted for consumers.
        note: String,
    },
}

impl Stability {
    pub(crate) const fn since(&self) -> Option<Version> {
        match self {
            Self::Stable { since } => Some(*since),
            Self::Deprecated { since, .. } => *since,
            Self::Unknown | Self::Unstable { .. } => None,
        }
    }

    pub(crate) const fn deprecated(&self) -> Option<Version> {
        match self {
            Self::Deprecated { deprecated, .. } => Some(*deprecated),
            Self::Unknown | Self::Unstable { .. } | Self::Stable { .. } => None,
        }
    }

    pub(crate) fn unstable_feature(&self) -> Option<&str> {
        match self {
            Self::Unstable { feature } => Some(feature),
            Self::Deprecated { feature, .. } => feature.as_deref(),
            Self::Unknown | Self::Stable { .. } => None,
        }
    }

    pub(crate) fn deprecation_note(&self) -> Option<&str> {
        match self {
            Self::Deprecated { note, .. } => Some(note),
            Self::Unknown | Self::Unstable { .. } | Self::Stable { .. } => None,
        }
    }
}

pub(crate) struct SinceHistory {
    available: BTreeSet<Version>,
    first_seen: BTreeMap<String, Version>,
    reported: BTreeSet<(String, Version)>,
}

impl SinceHistory {
    pub(crate) fn new(available: impl IntoIterator<Item = Version>) -> Self {
        Self {
            available: available.into_iter().collect(),
            first_seen: BTreeMap::new(),
            reported: BTreeSet::new(),
        }
    }

    pub(crate) fn check(&mut self, model: &Model) -> Vec<(String, String)> {
        let mut errors = Vec::new();
        let mut present = BTreeSet::new();
        for interface in &model.interfaces {
            let interface_path = interface.name.clone();
            self.check_item(
                &interface_path,
                model.package.version,
                &interface.stability,
                &mut present,
                &mut errors,
            );
            for def in &interface.types {
                self.check_item(
                    &format!("{interface_path}/{}", def.name),
                    model.package.version,
                    &def.stability,
                    &mut present,
                    &mut errors,
                );
            }
            for function in &interface.functions {
                self.check_item(
                    &format!("{interface_path}#{}", function.name),
                    model.package.version,
                    &function.stability,
                    &mut present,
                    &mut errors,
                );
            }
        }
        for world in &model.worlds {
            let world_path = world.name.clone();
            self.check_item(
                &world_path,
                model.package.version,
                &world.stability,
                &mut present,
                &mut errors,
            );
            for (side, interfaces) in [("import", &world.imports), ("export", &world.exports)] {
                for interface in interfaces {
                    self.check_item(
                        &format!("{world_path}/{side}/{}", interface.qualified_name()),
                        model.package.version,
                        &interface.stability,
                        &mut present,
                        &mut errors,
                    );
                }
            }
        }
        self.first_seen.retain(|path, _| present.contains(path));
        errors
    }

    fn check_item(
        &mut self,
        path: &str,
        current: Version,
        stability: &Stability,
        present: &mut BTreeSet<String>,
        errors: &mut Vec<(String, String)>,
    ) {
        if matches!(
            stability,
            Stability::Unstable { .. }
                | Stability::Deprecated {
                    since: None,
                    feature: Some(_),
                    ..
                }
        ) {
            return;
        }
        present.insert(path.to_string());
        let first = *self.first_seen.entry(path.to_string()).or_insert(current);
        let Some(since) = stability.since() else {
            return;
        };
        if self.available.contains(&since)
            && first != since
            && self.reported.insert((path.to_string(), since))
        {
            errors.push((
                path.to_string(),
                format!(
                    "`{path}` declares `@since(version = {since})`, but its first declaration in this root is version {first}"
                ),
            ));
        }
    }
}

/// A type reference to a named type in the contract package or a standard `deps/` dependency.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub struct QualifiedName {
    /// The defining interface.
    pub interface: String,
    /// The type name within the interface.
    pub name: String,
}

impl fmt::Display for QualifiedName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.interface, self.name)
    }
}

impl QualifiedName {
    /// Create a reference to a named type.
    #[must_use]
    pub fn new(interface: String, name: String) -> Self {
        Self { interface, name }
    }

    /// The package identity when this name belongs to a dependency, as
    /// `(namespace, package)`; `None` for a name in the root package.
    #[must_use]
    pub fn package(&self) -> Option<(&str, &str)> {
        let (package, _) = self.interface.split_once('/')?;
        package.split_once(':')
    }

    /// The interface component without a dependency package prefix.
    #[must_use]
    pub fn interface_name(&self) -> &str {
        self.interface
            .split_once('/')
            .map_or(self.interface.as_str(), |(_, interface)| interface)
    }

    /// Whether this name belongs to a dependency package.
    #[must_use]
    pub fn is_foreign(&self) -> bool {
        self.package().is_some()
    }
}

/// A type expression in the model.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum TypeExpr {
    /// `bool`.
    Bool,
    /// `u8`.
    U8,
    /// `u16`.
    U16,
    /// `u32`.
    U32,
    /// `u64`.
    U64,
    /// `s8`.
    S8,
    /// `s16`.
    S16,
    /// `s32`.
    S32,
    /// `s64`.
    S64,
    /// `f32`.
    F32,
    /// `f64`.
    F64,
    /// `string`.
    String,
    /// A named type, including an alias or named structural type.
    Named(QualifiedName),
    /// `list<T>`.
    List(Box<Self>),
    /// `option<T>`.
    Option(Box<Self>),
    /// `result<ok, err>`, with `None` payloads rendered as `_`.
    Result(ResultExpr),
}

/// The two payload slots of a `result`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct ResultExpr {
    /// The success payload, or `None` for `_`.
    pub ok: Option<Box<TypeExpr>>,
    /// The error payload, or `None` for `_`.
    pub err: Option<Box<TypeExpr>>,
}

impl ResultExpr {
    /// Create the payload slots of a `result`.
    #[must_use]
    pub fn new(ok: Option<Box<TypeExpr>>, err: Option<Box<TypeExpr>>) -> Self {
        Self { ok, err }
    }
}

/// A record field.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Field {
    /// The field name.
    pub name: String,
    /// Documentation from WIT.
    pub docs: String,
    /// The field type.
    pub ty: TypeExpr,
}

impl Field {
    /// Create a record field.
    #[must_use]
    pub fn new(name: String, docs: String, ty: TypeExpr) -> Self {
        Self { name, docs, ty }
    }
}

/// A variant or enum case.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Case {
    /// The case name.
    pub name: String,
    /// Documentation from WIT.
    pub docs: String,
    /// The variant payload; `None` for a payload-less case or an enum case.
    pub payload: Option<TypeExpr>,
}

impl Case {
    /// Create a variant or enum case.
    #[must_use]
    pub fn new(name: String, docs: String, payload: Option<TypeExpr>) -> Self {
        Self {
            name,
            docs,
            payload,
        }
    }
}

/// A flags member.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Flag {
    /// The flag name.
    pub name: String,
    /// Documentation from WIT.
    pub docs: String,
}

impl Flag {
    /// Create a flags member.
    #[must_use]
    pub fn new(name: String, docs: String) -> Self {
        Self { name, docs }
    }
}

/// The definition of a named type in an interface.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct TypeDef {
    /// The type name.
    pub name: String,
    /// Documentation from WIT.
    pub docs: String,
    /// Stability metadata from WIT.
    pub stability: Stability,
    /// The type's shape.
    pub kind: TypeDefKind,
}

impl TypeDef {
    /// Create a named type definition.
    #[must_use]
    pub fn new(name: String, docs: String, stability: Stability, kind: TypeDefKind) -> Self {
        Self {
            name,
            docs,
            stability,
            kind,
        }
    }
}

/// The shape of a named type.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum TypeDefKind {
    /// A record.
    Record(Vec<Field>),
    /// A variant.
    Variant(Vec<Case>),
    /// An enum.
    Enum(Vec<Case>),
    /// A flags type.
    Flags(Vec<Flag>),
    /// A `type a = b` or `use` alias.
    Alias(TypeExpr),
    /// A named `list<T>` definition.
    List(TypeExpr),
    /// A named `option<T>` definition.
    Option(TypeExpr),
    /// A named `result<ok, err>` definition.
    Result(ResultExpr),
}

/// A function parameter.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Param {
    /// The parameter name.
    pub name: String,
    /// The parameter type.
    pub ty: TypeExpr,
}

impl Param {
    /// Create a function parameter.
    #[must_use]
    pub fn new(name: String, ty: TypeExpr) -> Self {
        Self { name, ty }
    }
}

/// An interface function.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Function {
    /// The unmangled WIT item name.
    pub name: String,
    /// Documentation from WIT.
    pub docs: String,
    /// Stability metadata from WIT.
    pub stability: Stability,
    /// Parameters in source order.
    pub params: Vec<Param>,
    /// The result type, or `None` for no result.
    pub result: Option<TypeExpr>,
}

impl Function {
    /// Create an interface function.
    #[must_use]
    pub fn new(
        name: String,
        docs: String,
        stability: Stability,
        params: Vec<Param>,
        result: Option<TypeExpr>,
    ) -> Self {
        Self {
            name,
            docs,
            stability,
            params,
            result,
        }
    }
}

/// A named interface and its definitions.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Interface {
    /// The interface name.
    pub name: String,
    /// Documentation from WIT.
    pub docs: String,
    /// Stability metadata from WIT.
    pub stability: Stability,
    /// Types in source order.
    pub types: Vec<TypeDef>,
    /// Functions in source order.
    pub functions: Vec<Function>,
}

impl Interface {
    /// Create a named interface.
    #[must_use]
    pub fn new(
        name: String,
        docs: String,
        stability: Stability,
        types: Vec<TypeDef>,
        functions: Vec<Function>,
    ) -> Self {
        Self {
            name,
            docs,
            stability,
            types,
            functions,
        }
    }
}

/// A named interface imported or exported by a world.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct WorldInterface {
    /// The interface name within its package.
    pub interface: String,
    /// The defining dependency package, or `None` for an interface in the root package.
    pub package: Option<Package>,
    /// Documentation on the import or export statement.
    pub docs: String,
    /// Stability metadata on the import or export statement.
    pub stability: Stability,
}

impl WorldInterface {
    /// Create a world import or export of a named interface.
    #[must_use]
    pub fn new(
        interface: String,
        package: Option<Package>,
        docs: String,
        stability: Stability,
    ) -> Self {
        Self {
            interface,
            package,
            docs,
            stability,
        }
    }

    /// The package-qualified interface name, as `namespace:package/interface` for a dependency
    /// and `interface` for a local interface.
    #[must_use]
    pub fn qualified_name(&self) -> String {
        match &self.package {
            Some(package) => format!("{}:{}/{}", package.namespace, package.name, self.interface),
            None => self.interface.clone(),
        }
    }
}

/// A world and its imported and exported interfaces.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct World {
    /// The world name.
    pub name: String,
    /// Documentation from WIT.
    pub docs: String,
    /// Stability metadata from WIT.
    pub stability: Stability,
    /// Imported interfaces in source order.
    pub imports: Vec<WorldInterface>,
    /// Exported interfaces in source order.
    pub exports: Vec<WorldInterface>,
}

impl World {
    /// Create a world.
    #[must_use]
    pub fn new(
        name: String,
        docs: String,
        stability: Stability,
        imports: Vec<WorldInterface>,
        exports: Vec<WorldInterface>,
    ) -> Self {
        Self {
            name,
            docs,
            stability,
            imports,
            exports,
        }
    }
}

/// The contract package metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Package {
    /// The package namespace.
    pub namespace: String,
    /// The package name.
    pub name: String,
    /// The package version; contract versions are `major.minor.0`.
    pub version: Version,
    /// Documentation from WIT.
    pub docs: String,
}

impl Package {
    /// Create contract package metadata.
    #[must_use]
    pub fn new(namespace: String, name: String, version: Version, docs: String) -> Self {
        Self {
            namespace,
            name,
            version,
            docs,
        }
    }
}

/// One resolved dependency package from the standard `deps/` layout.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Dependency {
    /// The dependency package metadata.
    pub package: Package,
    /// Interfaces in source order.
    pub interfaces: Vec<Interface>,
}

impl Dependency {
    /// Create a resolved dependency package.
    #[must_use]
    pub fn new(package: Package, interfaces: Vec<Interface>) -> Self {
        Self {
            package,
            interfaces,
        }
    }
}

/// The owned model of one contract version.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Model {
    /// The package metadata.
    pub package: Package,
    /// Interfaces in source order.
    pub interfaces: Vec<Interface>,
    /// Worlds in source order.
    pub worlds: Vec<World>,
    /// Standard `deps/` packages, sorted by package name.
    pub dependencies: Vec<Dependency>,
}

impl Model {
    /// Create an owned contract model.
    #[must_use]
    pub fn new(
        package: Package,
        interfaces: Vec<Interface>,
        worlds: Vec<World>,
        dependencies: Vec<Dependency>,
    ) -> Self {
        Self {
            package,
            interfaces,
            worlds,
            dependencies,
        }
    }

    /// Find an interface by name.
    #[must_use]
    pub fn interface(&self, name: &str) -> Option<&Interface> {
        self.interfaces
            .iter()
            .find(|interface| interface.name == name)
    }

    /// Find a dependency package by its `(namespace, package)` identity.
    #[must_use]
    pub fn dependency_package(&self, namespace: &str, package: &str) -> Option<&Dependency> {
        self.dependencies.iter().find(|dependency| {
            dependency.package.namespace == namespace && dependency.package.name == package
        })
    }

    /// Find a named type by its qualified name, including dependency types.
    #[must_use]
    pub fn type_def(&self, name: &QualifiedName) -> Option<&TypeDef> {
        self.type_def_in(None, name).map(|(_, def)| def)
    }

    /// Resolve a named type, using `context` for a dependency-local name that has no package
    /// prefix.
    pub(crate) fn type_def_in<'a>(
        &'a self,
        context: Option<&'a Dependency>,
        name: &QualifiedName,
    ) -> Option<(Option<&'a Dependency>, &'a TypeDef)> {
        if let Some((namespace, package)) = name.package() {
            let dependency = self.dependency_package(namespace, package)?;
            let def = dependency
                .interfaces
                .iter()
                .find(|interface| interface.name == name.interface_name())?
                .types
                .iter()
                .find(|def| def.name == name.name)?;
            return Some((Some(dependency), def));
        }
        if let Some(dependency) = context {
            let def = dependency
                .interfaces
                .iter()
                .find(|interface| interface.name == name.interface)?
                .types
                .iter()
                .find(|def| def.name == name.name)?;
            return Some((Some(dependency), def));
        }
        let def = self
            .interface(&name.interface)?
            .types
            .iter()
            .find(|def| def.name == name.name)?;
        Some((None, def))
    }

    /// Whether `ty` resolves to `u8` after following aliases, including aliases declared in a
    /// dependency package.
    ///
    /// This is the decision the byte-list projection uses: a list whose resolved element type is
    /// `u8` is protobuf `bytes`, whatever spelling the contract used. A missing or cyclic alias
    /// resolves to `false`; the emitter reports the dangling type or cycle when it renders it.
    #[must_use]
    pub(crate) fn resolves_to_u8(&self, ty: &TypeExpr) -> bool {
        self.resolves_to_u8_in(None, ty)
    }

    /// Like [`Model::resolves_to_u8`], resolving an unqualified alias inside `context`'s
    /// dependency package.
    #[must_use]
    pub(crate) fn resolves_to_u8_in(&self, context: Option<&Dependency>, ty: &TypeExpr) -> bool {
        self.resolves_to_u8_inner(context, ty, &mut HashSet::new())
    }

    fn resolves_to_u8_inner(
        &self,
        context: Option<&Dependency>,
        ty: &TypeExpr,
        visiting: &mut HashSet<String>,
    ) -> bool {
        let TypeExpr::Named(name) = ty else {
            return matches!(ty, TypeExpr::U8);
        };
        let Some((resolved, def)) = self.type_def_in(context, name) else {
            return false;
        };
        let TypeDefKind::Alias(target) = &def.kind else {
            return false;
        };
        let key = resolved.map_or_else(
            || format!("{}/{}", name.interface, name.name),
            |dependency| {
                format!(
                    "{}:{}/{}/{}",
                    dependency.package.namespace,
                    dependency.package.name,
                    name.interface_name(),
                    name.name
                )
            },
        );
        if !visiting.insert(key.clone()) {
            return false;
        }
        let answer = self.resolves_to_u8_inner(resolved, target, visiting);
        visiting.remove(&key);
        answer
    }

    /// Render `ty` in the canonical wire form used by the lockfile.
    ///
    /// Named references resolve through this model: aliases expand recursively, named
    /// `list`/`option`/`result` definitions expand structurally, and record, variant, enum, and
    /// flags types are written as `interface/type`. The result contains no whitespace.
    ///
    /// # Errors
    ///
    /// Returns an error if `ty` names a type that is not in this model, or if aliases or named
    /// structural definitions form a cycle.
    pub fn canonical_type(&self, ty: &TypeExpr) -> Result<String> {
        self.canonical_type_inner(ty, &mut HashSet::new(), 0, 0)
    }

    fn canonical_type_inner(
        &self,
        ty: &TypeExpr,
        visiting: &mut HashSet<QualifiedName>,
        nesting: usize,
        aliases: usize,
    ) -> Result<String> {
        let text = match ty {
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
            TypeExpr::Named(name) => self.canonical_named(name, visiting, nesting, aliases)?,
            TypeExpr::List(element) => {
                let nesting = next_nesting(nesting)?;
                format!(
                    "list<{}>",
                    self.canonical_type_inner(element, visiting, nesting, aliases)?
                )
            }
            TypeExpr::Option(payload) => {
                let nesting = next_nesting(nesting)?;
                format!(
                    "option<{}>",
                    self.canonical_type_inner(payload, visiting, nesting, aliases)?
                )
            }
            TypeExpr::Result(result) => {
                let nesting = next_nesting(nesting)?;
                self.canonical_result(result, visiting, nesting, aliases)?
            }
        };
        check_canonical_length(text)
    }

    fn canonical_result(
        &self,
        result: &ResultExpr,
        visiting: &mut HashSet<QualifiedName>,
        nesting: usize,
        aliases: usize,
    ) -> Result<String> {
        let ok = match &result.ok {
            Some(ty) => self.canonical_type_inner(ty, visiting, nesting, aliases)?,
            None => "_".to_string(),
        };
        let err = match &result.err {
            Some(ty) => self.canonical_type_inner(ty, visiting, nesting, aliases)?,
            None => "_".to_string(),
        };
        Ok(format!("result<{ok},{err}>"))
    }

    fn canonical_named(
        &self,
        name: &QualifiedName,
        visiting: &mut HashSet<QualifiedName>,
        nesting: usize,
        aliases: usize,
    ) -> Result<String> {
        if name.is_foreign() {
            return Ok(name.to_string());
        }
        if !visiting.insert(name.clone()) {
            bail!("canonical type cycle at `{name}`");
        }
        let result = match self.type_def(name) {
            Some(TypeDef {
                kind:
                    TypeDefKind::Record(_)
                    | TypeDefKind::Variant(_)
                    | TypeDefKind::Enum(_)
                    | TypeDefKind::Flags(_),
                ..
            }) => Ok(name.to_string()),
            Some(TypeDef {
                kind: TypeDefKind::Alias(target),
                ..
            }) => {
                if aliases >= MAX_ALIAS_CHAIN {
                    bail!("alias chain at `{name}` exceeds {MAX_ALIAS_CHAIN} links");
                }
                self.canonical_type_inner(target, visiting, nesting, aliases + 1)
            }
            Some(TypeDef {
                kind: TypeDefKind::List(element),
                ..
            }) => {
                let nesting = next_nesting(nesting)?;
                Ok(format!(
                    "list<{}>",
                    self.canonical_type_inner(element, visiting, nesting, aliases)?
                ))
            }
            Some(TypeDef {
                kind: TypeDefKind::Option(payload),
                ..
            }) => {
                let nesting = next_nesting(nesting)?;
                Ok(format!(
                    "option<{}>",
                    self.canonical_type_inner(payload, visiting, nesting, aliases)?
                ))
            }
            Some(TypeDef {
                kind: TypeDefKind::Result(result),
                ..
            }) => {
                let nesting = next_nesting(nesting)?;
                self.canonical_result(result, visiting, nesting, aliases)
            }
            None => bail!("type `{name}` is not in the model"),
        };
        visiting.remove(name);
        result
    }
}

fn next_nesting(nesting: usize) -> Result<usize> {
    let next = nesting + 1;
    if next > MAX_TYPE_NESTING {
        bail!("type nesting exceeds {MAX_TYPE_NESTING} levels");
    }
    Ok(next)
}

fn check_canonical_length(text: String) -> Result<String> {
    if text.len() > MAX_CANONICAL_TYPE_BYTES {
        bail!(
            "canonical type is {} bytes, over the {MAX_CANONICAL_TYPE_BYTES}-byte limit",
            text.len()
        );
    }
    Ok(text)
}

impl TypeExpr {
    /// Render this type in canonical wire form using `model`.
    ///
    /// This is a convenience wrapper around [`Model::canonical_type`].
    ///
    /// # Errors
    ///
    /// Returns the same errors as [`Model::canonical_type`].
    pub fn canonical(&self, model: &Model) -> Result<String> {
        model.canonical_type(self)
    }
}

/// Refuse a dependency graph that gives one package identity two versions, or that reuses the
/// root package's namespace and name.
///
/// Type identity is `namespace:name/interface/type`; the dependency version is not part of a
/// lockfile key or canonical type. Two versions under one `deps/` tree would therefore collapse
/// into one package in the lockfile and every emitter. One graph must pin one version, and a new
/// major is adopted by every dependent at once.
///
/// The diagnostic names the package, every version found, and the `deps/` entry that supplied
/// each copy. It is a [`crate::ContractError`], so `check`, `lock`, `diff`, and every generator
/// report exit `1` from the shared loader.
///
/// # Errors
///
/// Returns a contract error when a dependency has the root's namespace and name, or when more
/// than one dependency package has one namespace and name.
pub(crate) fn ensure_dependency_identity(
    root: &PackageName,
    dependencies: &[(PackageName, PathBuf)],
) -> Result<()> {
    let mut groups: BTreeMap<(String, String), Vec<(String, String)>> = BTreeMap::new();
    for (package, entry) in dependencies {
        if package.namespace == root.namespace && package.name == root.name {
            let pkg_id = format!("{}:{}", package.namespace, package.name);
            bail!(crate::ContractError::with_diagnostic(
                crate::diagnostic::Diagnostic::error(
                    crate::diagnostic::codes::SUBSET_DEPENDENCY_IDENTITY,
                    format!(
                        "dependency package `{pkg_id}` has the root package's namespace:name; the \
                         root and a dependency cannot share a package identity"
                    ),
                )
                .with_path(pkg_id)
            ));
        }
        let version = package
            .version
            .as_ref()
            .map_or_else(|| "<unversioned>".to_string(), ToString::to_string);
        groups
            .entry((package.namespace.clone(), package.name.clone()))
            .or_default()
            .push((version, entry.display().to_string()));
    }
    for ((namespace, name), copies) in groups {
        if copies.len() > 1 {
            let mut versions: Vec<&str> =
                copies.iter().map(|(version, _)| version.as_str()).collect();
            versions.sort_unstable();
            versions.dedup();
            let versions = if versions.len() == 1 {
                format!("version {}", versions[0])
            } else {
                format!("versions {}", versions.join(", "))
            };
            let entries = copies
                .iter()
                .map(|(_, entry)| format!("`{entry}`"))
                .collect::<Vec<_>>()
                .join(", ");
            let pkg_id = format!("{namespace}:{name}");
            let file = copies
                .first()
                .map(|(_, entry)| entry.clone())
                .unwrap_or_default();
            let diag = crate::diagnostic::Diagnostic::error(
                crate::diagnostic::codes::SUBSET_DEPENDENCY_IDENTITY,
                format!(
                    "duplicate dependency package `{pkg_id}`: {versions} in {entries}; one \
                     dependency graph must pin one version, and a new major is adopted by \
                     every dependent at once"
                ),
            )
            .with_path(pkg_id.clone());
            let mut located =
                crate::diagnostic::LocatedDiagnostic::from_diagnostic(diag).with_logical(pkg_id);
            if !file.is_empty() {
                located = located.with_file(file);
            }
            bail!(crate::ContractError::with_located_diagnostic(located));
        }
    }
    Ok(())
}

/// Build the owned model from a loaded contract.
///
/// The subset checker is run first. A contract outside the subset is an error rather than a
/// partially populated model, and a construct this conversion does not recognise is also an
/// error rather than silently dropped.
///
/// # Errors
///
/// Returns an error for a subset violation, an unsupported `wit-parser` construct, a
/// world-defined type, a non-freestanding function, or a contract version that is not
/// `major.minor.0`. Unsupported items in dependency interfaces are skipped only when no root type
/// or world reaches them; the subset checker traverses every required dependency definition before
/// this conversion starts.
pub fn build(contract: &Contract) -> Result<Model> {
    let violations = crate::subset::check_located_diagnostics(contract);
    if !violations.is_empty() {
        let messages: Vec<String> = violations
            .iter()
            .map(|located| match &located.diagnostic.path {
                Some(path) => format!("{path}: {}", located.diagnostic.message),
                None => located.diagnostic.message.clone(),
            })
            .collect();
        bail!(crate::ContractError::with_located_diagnostics(
            format!(
                "contract does not satisfy the subset: {}",
                messages.join("; ")
            ),
            violations,
        ));
    }
    let model = build_unchecked(contract)?;
    model.validate_type_limits()?;
    Ok(model)
}

/// Build the owned model without re-running `subset::check` or `validate_type_limits`.
///
/// Unreachable unsupported dependency items—and every dependency item that transitively
/// references them, including across dependency packages—are pruned to a fixed point so no
/// emitter ever sees a dangling dependency reference.
pub(crate) fn build_unchecked(contract: &Contract) -> Result<Model> {
    let resolve = &contract.resolve;
    let root = contract.package;
    let wit_package = &resolve.packages[root];
    let wit_version = wit_package
        .name
        .version
        .as_ref()
        .context("the contract package has no version")?;
    ensure!(
        wit_version.patch == 0
            && wit_version.pre.as_str().is_empty()
            && wit_version.build.as_str().is_empty(),
        "contract versions must be `<major>.<minor>.0`"
    );
    let package = Package {
        namespace: wit_package.name.namespace.clone(),
        name: wit_package.name.name.clone(),
        version: Version {
            major: wit_version.major,
            minor: wit_version.minor,
            patch: wit_version.patch,
        },
        docs: docs(&wit_package.docs),
    };

    let mut interface_ids: Vec<_> = wit_package.interfaces.iter().collect();
    interface_ids.sort_by_key(|(_, id)| source_key(resolve.interfaces[**id].span));
    let interfaces = interface_ids
        .into_iter()
        .map(|(name, id)| build_interface(resolve, root, name, *id))
        .collect::<Result<Vec<_>>>()?;

    let mut world_ids: Vec<_> = wit_package.worlds.iter().collect();
    world_ids.sort_by_key(|(_, id)| source_key(resolve.worlds[**id].span));
    let worlds = world_ids
        .into_iter()
        .map(|(name, id)| build_world(resolve, root, name, *id))
        .collect::<Result<Vec<_>>>()?;

    let mut dependencies = Vec::new();
    for package_id in &contract.dependencies {
        let wit_package = &resolve.packages[*package_id];
        let dep_pkg = format!("{}:{}", wit_package.name.namespace, wit_package.name.name);
        let wit_version = wit_package
            .name
            .version
            .as_ref()
            .with_context(|| format!("dependency package `{dep_pkg}` has no version"))?;
        ensure!(
            wit_version.patch == 0
                && wit_version.pre.as_str().is_empty()
                && wit_version.build.as_str().is_empty(),
            "dependency package `{dep_pkg}@{wit_version}` has unsupported version \
             `{wit_version}`; dependency versions must be `<major>.<minor>.0`"
        );
        let dependency_package = Package {
            namespace: wit_package.name.namespace.clone(),
            name: wit_package.name.name.clone(),
            version: Version {
                major: wit_version.major,
                minor: wit_version.minor,
                patch: wit_version.patch,
            },
            docs: docs(&wit_package.docs),
        };
        let mut interface_ids: Vec<_> = wit_package.interfaces.iter().collect();
        interface_ids.sort_by_key(|(_, id)| source_key(resolve.interfaces[**id].span));
        let dependency_interfaces = interface_ids
            .into_iter()
            .map(|(name, id)| build_dependency_interface(resolve, *package_id, name, *id))
            .collect();
        dependencies.push(Dependency {
            package: dependency_package,
            interfaces: dependency_interfaces,
        });
    }
    dependencies.sort_by(|left, right| {
        (&left.package.namespace, &left.package.name)
            .cmp(&(&right.package.namespace, &right.package.name))
    });
    prune_dangling_dependency_items(&mut dependencies);
    Ok(Model {
        package,
        interfaces,
        worlds,
        dependencies,
    })
}

/// Iteratively remove any dependency type or function that references a skipped dependency type,
/// within the same dependency package or across dependency packages.
fn prune_dangling_dependency_items(dependencies: &mut [Dependency]) {
    loop {
        let mut surviving: HashSet<(String, String, String, String)> = HashSet::new();
        for dep in dependencies.iter() {
            for iface in &dep.interfaces {
                for def in &iface.types {
                    surviving.insert((
                        dep.package.namespace.clone(),
                        dep.package.name.clone(),
                        iface.name.clone(),
                        def.name.clone(),
                    ));
                }
            }
        }
        let mut removed_any = false;
        for dep in dependencies.iter_mut() {
            let ns = dep.package.namespace.clone();
            let pkg = dep.package.name.clone();
            for iface in &mut dep.interfaces {
                let before_types = iface.types.len();
                iface
                    .types
                    .retain(|def| type_def_refs_exist(&ns, &pkg, def, &surviving));
                if iface.types.len() != before_types {
                    removed_any = true;
                }
                let before_fns = iface.functions.len();
                iface
                    .functions
                    .retain(|func| function_refs_exist(&ns, &pkg, func, &surviving));
                if iface.functions.len() != before_fns {
                    removed_any = true;
                }
            }
        }
        if !removed_any {
            break;
        }
    }
}

fn type_def_refs_exist(
    ns: &str,
    pkg: &str,
    def: &TypeDef,
    surviving: &HashSet<(String, String, String, String)>,
) -> bool {
    match &def.kind {
        TypeDefKind::Record(fields) => fields
            .iter()
            .all(|field| type_expr_refs_exist(ns, pkg, &field.ty, surviving)),
        TypeDefKind::Variant(cases) => cases.iter().all(|case| {
            case.payload
                .as_ref()
                .is_none_or(|ty| type_expr_refs_exist(ns, pkg, ty, surviving))
        }),
        TypeDefKind::Enum(_) | TypeDefKind::Flags(_) => true,
        TypeDefKind::Alias(ty) | TypeDefKind::List(ty) | TypeDefKind::Option(ty) => {
            type_expr_refs_exist(ns, pkg, ty, surviving)
        }
        TypeDefKind::Result(result) => {
            result
                .ok
                .as_deref()
                .is_none_or(|ty| type_expr_refs_exist(ns, pkg, ty, surviving))
                && result
                    .err
                    .as_deref()
                    .is_none_or(|ty| type_expr_refs_exist(ns, pkg, ty, surviving))
        }
    }
}

fn function_refs_exist(
    ns: &str,
    pkg: &str,
    func: &Function,
    surviving: &HashSet<(String, String, String, String)>,
) -> bool {
    func.params
        .iter()
        .all(|param| type_expr_refs_exist(ns, pkg, &param.ty, surviving))
        && func
            .result
            .as_ref()
            .is_none_or(|ty| type_expr_refs_exist(ns, pkg, ty, surviving))
}

fn type_expr_refs_exist(
    ns: &str,
    pkg: &str,
    ty: &TypeExpr,
    surviving: &HashSet<(String, String, String, String)>,
) -> bool {
    match ty {
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
        | TypeExpr::String => true,
        TypeExpr::List(inner) | TypeExpr::Option(inner) => {
            type_expr_refs_exist(ns, pkg, inner, surviving)
        }
        TypeExpr::Result(result) => {
            result
                .ok
                .as_deref()
                .is_none_or(|inner| type_expr_refs_exist(ns, pkg, inner, surviving))
                && result
                    .err
                    .as_deref()
                    .is_none_or(|inner| type_expr_refs_exist(ns, pkg, inner, surviving))
        }
        TypeExpr::Named(name) => {
            let (target_ns, target_pkg) = name.package().unwrap_or((ns, pkg));
            surviving.contains(&(
                target_ns.to_string(),
                target_pkg.to_string(),
                name.interface_name().to_string(),
                name.name.clone(),
            ))
        }
    }
}

impl Model {
    /// Collect all type-expansion limit violations (`MAX_TYPE_NESTING`, `MAX_ALIAS_CHAIN`,
    /// `MAX_CANONICAL_TYPE_BYTES`) across the root package and every dependency package.
    pub(crate) fn collect_type_limit_errors(&self) -> Vec<(String, String)> {
        let mut errors = Vec::new();
        let root_pkg = format!("{}:{}", self.package.namespace, self.package.name);
        for interface in &self.interfaces {
            self.collect_interface_limit_errors(&root_pkg, interface, &mut errors);
        }
        for dependency in &self.dependencies {
            let dep_pkg = format!(
                "{}:{}",
                dependency.package.namespace, dependency.package.name
            );
            let model = Model {
                package: dependency.package.clone(),
                interfaces: dependency.interfaces.clone(),
                worlds: Vec::new(),
                dependencies: self
                    .dependencies
                    .iter()
                    .filter(|other| {
                        other.package.namespace != dependency.package.namespace
                            || other.package.name != dependency.package.name
                    })
                    .cloned()
                    .collect(),
            };
            for interface in &model.interfaces {
                model.collect_interface_limit_errors(&dep_pkg, interface, &mut errors);
            }
        }
        errors
    }

    fn collect_interface_limit_errors(
        &self,
        pkg: &str,
        interface: &Interface,
        errors: &mut Vec<(String, String)>,
    ) {
        for def in &interface.types {
            let base = format!("{pkg}/{}/{}", interface.name, def.name);
            let context = || format!("type `{}/{}`", interface.name, def.name);
            match &def.kind {
                TypeDefKind::Record(fields) => {
                    for field in fields {
                        if let Err(err) = self
                            .canonical_type(&field.ty)
                            .with_context(|| format!("{} field `{}`", context(), field.name))
                        {
                            errors.push((format!("{base}.{}", field.name), format!("{err:#}")));
                        }
                    }
                }
                TypeDefKind::Variant(cases) => {
                    for case in cases {
                        if let Some(payload) = &case.payload
                            && let Err(err) = self
                                .canonical_type(payload)
                                .with_context(|| format!("{} case `{}`", context(), case.name))
                        {
                            errors.push((format!("{base}.{}", case.name), format!("{err:#}")));
                        }
                    }
                }
                TypeDefKind::Alias(_)
                | TypeDefKind::List(_)
                | TypeDefKind::Option(_)
                | TypeDefKind::Result(_) => {
                    let name = QualifiedName {
                        interface: interface.name.clone(),
                        name: def.name.clone(),
                    };
                    if let Err(err) = self
                        .canonical_type(&TypeExpr::Named(name))
                        .with_context(context)
                    {
                        errors.push((base, format!("{err:#}")));
                    }
                }
                TypeDefKind::Enum(_) | TypeDefKind::Flags(_) => {}
            }
        }
        for function in &interface.functions {
            for param in &function.params {
                if let Err(err) = self.canonical_type(&param.ty).with_context(|| {
                    format!(
                        "type `{}/{}` parameter `{}`",
                        interface.name, function.name, param.name
                    )
                }) {
                    errors.push((
                        format!("{pkg}/{}#{}({})", interface.name, function.name, param.name),
                        format!("{err:#}"),
                    ));
                }
            }
            if let Some(result) = &function.result
                && let Err(err) = self
                    .canonical_type(result)
                    .with_context(|| format!("type `{}/{}` result", interface.name, function.name))
            {
                errors.push((
                    format!("{pkg}/{}#{} -> result", interface.name, function.name),
                    format!("{err:#}"),
                ));
            }
        }
    }

    /// Refuse an alias chain, nested type, or canonical string over the documented limits.
    ///
    /// This runs for every definition and function in the root package and in every dependency
    /// package, including definitions nothing refers to, so `check`, `lock`, `diff`, and every
    /// generator see the same bound before they can recurse.
    fn validate_type_limits(&self) -> Result<()> {
        for interface in &self.interfaces {
            self.validate_interface_limits(interface)?;
        }
        for dependency in &self.dependencies {
            let model = Model {
                package: dependency.package.clone(),
                interfaces: dependency.interfaces.clone(),
                worlds: Vec::new(),
                dependencies: self
                    .dependencies
                    .iter()
                    .filter(|other| {
                        other.package.namespace != dependency.package.namespace
                            || other.package.name != dependency.package.name
                    })
                    .cloned()
                    .collect(),
            };
            for interface in &model.interfaces {
                model.validate_interface_limits(interface)?;
            }
        }
        Ok(())
    }

    fn validate_interface_limits(&self, interface: &Interface) -> Result<()> {
        let pkg = format!("{}:{}", self.package.namespace, self.package.name);
        let limit_err = |path: String, err: anyhow::Error| {
            let message = format!("{err:#}");
            crate::ContractError::with_diagnostic(
                crate::diagnostic::Diagnostic::error(
                    crate::diagnostic::codes::SUBSET_TYPE_LIMIT,
                    message,
                )
                .with_path(path),
            )
        };
        for def in &interface.types {
            let context = || format!("type `{}/{}`", interface.name, def.name);
            let base_path = format!("{pkg}/{}/{}", interface.name, def.name);
            match &def.kind {
                TypeDefKind::Record(fields) => {
                    for field in fields {
                        self.canonical_type(&field.ty)
                            .with_context(|| format!("{} field `{}`", context(), field.name))
                            .map_err(|err| limit_err(format!("{base_path}.{}", field.name), err))?;
                    }
                }
                TypeDefKind::Variant(cases) => {
                    for case in cases {
                        if let Some(payload) = &case.payload {
                            self.canonical_type(payload)
                                .with_context(|| format!("{} case `{}`", context(), case.name))
                                .map_err(|err| {
                                    limit_err(format!("{base_path}.{}", case.name), err)
                                })?;
                        }
                    }
                }
                TypeDefKind::Alias(_)
                | TypeDefKind::List(_)
                | TypeDefKind::Option(_)
                | TypeDefKind::Result(_) => {
                    let name = QualifiedName {
                        interface: interface.name.clone(),
                        name: def.name.clone(),
                    };
                    self.canonical_type(&TypeExpr::Named(name))
                        .with_context(context)
                        .map_err(|err| limit_err(base_path.clone(), err))?;
                }
                TypeDefKind::Enum(_) | TypeDefKind::Flags(_) => {}
            }
        }
        for function in &interface.functions {
            let fn_path = format!("{pkg}/{}#{}", interface.name, function.name);
            for param in &function.params {
                self.canonical_type(&param.ty)
                    .with_context(|| {
                        format!(
                            "type `{}/{}` parameter `{}`",
                            interface.name, function.name, param.name
                        )
                    })
                    .map_err(|err| limit_err(format!("{fn_path}({})", param.name), err))?;
            }
            if let Some(result) = &function.result {
                self.canonical_type(result)
                    .with_context(|| format!("type `{}/{}` result", interface.name, function.name))
                    .map_err(|err| limit_err(format!("{fn_path} -> result"), err))?;
            }
        }
        Ok(())
    }
}

fn build_dependency_interface(
    resolve: &Resolve,
    root: PackageId,
    name: &str,
    id: InterfaceId,
) -> Interface {
    let interface = &resolve.interfaces[id];
    let mut type_ids: Vec<_> = interface.types.iter().collect();
    type_ids.sort_by_key(|(_, id)| source_key(resolve.types[**id].span));
    let types = type_ids
        .into_iter()
        .filter_map(|(type_name, type_id)| build_type_def(resolve, root, type_name, *type_id).ok())
        .collect();
    let mut function_ids: Vec<_> = interface.functions.iter().collect();
    function_ids.sort_by_key(|(_, function)| source_key(function.span));
    let functions = function_ids
        .into_iter()
        .filter_map(|(function_name, function)| {
            build_function(resolve, root, function_name, function).ok()
        })
        .collect();
    Interface {
        name: name.to_string(),
        docs: docs(&interface.docs),
        stability: stability(&interface.stability),
        types,
        functions,
    }
}

fn build_interface(
    resolve: &Resolve,
    root: PackageId,
    name: &str,
    id: InterfaceId,
) -> Result<Interface> {
    let interface = &resolve.interfaces[id];
    let mut type_ids: Vec<_> = interface.types.iter().collect();
    type_ids.sort_by_key(|(_, id)| source_key(resolve.types[**id].span));
    let types = type_ids
        .into_iter()
        .map(|(type_name, type_id)| build_type_def(resolve, root, type_name, *type_id))
        .collect::<Result<Vec<_>>>()?;

    let mut function_ids: Vec<_> = interface.functions.iter().collect();
    function_ids.sort_by_key(|(_, function)| source_key(function.span));
    let functions = function_ids
        .into_iter()
        .map(|(function_name, function)| build_function(resolve, root, function_name, function))
        .collect::<Result<Vec<_>>>()?;
    Ok(Interface {
        name: name.to_string(),
        docs: docs(&interface.docs),
        stability: stability(&interface.stability),
        types,
        functions,
    })
}

fn build_type_def(resolve: &Resolve, root: PackageId, name: &str, id: TypeId) -> Result<TypeDef> {
    let def = &resolve.types[id];
    let kind = match &def.kind {
        WitTypeDefKind::Record(record) => TypeDefKind::Record(
            record
                .fields
                .iter()
                .map(|field| {
                    Ok(Field {
                        name: field.name.clone(),
                        docs: docs(&field.docs),
                        ty: type_expr(resolve, root, &field.ty)
                            .with_context(|| format!("record field `{name}.{}`", field.name))?,
                    })
                })
                .collect::<Result<Vec<_>>>()?,
        ),
        WitTypeDefKind::Variant(variant) => TypeDefKind::Variant(
            variant
                .cases
                .iter()
                .map(|case| {
                    Ok(Case {
                        name: case.name.clone(),
                        docs: docs(&case.docs),
                        payload: case
                            .ty
                            .as_ref()
                            .map(|ty| type_expr(resolve, root, ty))
                            .transpose()
                            .with_context(|| format!("variant case `{name}.{}`", case.name))?,
                    })
                })
                .collect::<Result<Vec<_>>>()?,
        ),
        WitTypeDefKind::Enum(enumeration) => TypeDefKind::Enum(
            enumeration
                .cases
                .iter()
                .map(|case| Case {
                    name: case.name.clone(),
                    docs: docs(&case.docs),
                    payload: None,
                })
                .collect(),
        ),
        WitTypeDefKind::Flags(flags) => TypeDefKind::Flags(
            flags
                .flags
                .iter()
                .map(|flag| Flag {
                    name: flag.name.clone(),
                    docs: docs(&flag.docs),
                })
                .collect(),
        ),
        WitTypeDefKind::Type(target) => TypeDefKind::Alias(
            type_expr(resolve, root, target).with_context(|| format!("alias `{name}`"))?,
        ),
        WitTypeDefKind::List(element) => TypeDefKind::List(
            type_expr(resolve, root, element).with_context(|| format!("named list `{name}`"))?,
        ),
        WitTypeDefKind::Option(payload) => TypeDefKind::Option(
            type_expr(resolve, root, payload).with_context(|| format!("named option `{name}`"))?,
        ),
        WitTypeDefKind::Result(result) => TypeDefKind::Result(
            result_expr(resolve, root, result.ok.as_ref(), result.err.as_ref())
                .with_context(|| format!("named result `{name}`"))?,
        ),
        WitTypeDefKind::Resource
        | WitTypeDefKind::Handle(_)
        | WitTypeDefKind::Tuple(_)
        | WitTypeDefKind::Map(..)
        | WitTypeDefKind::FixedLengthList(..)
        | WitTypeDefKind::Future(_)
        | WitTypeDefKind::Stream(_)
        | WitTypeDefKind::Unknown => bail!("type `{name}` uses an unsupported WIT construct"),
    };
    Ok(TypeDef {
        name: name.to_string(),
        docs: docs(&def.docs),
        stability: stability(&def.stability),
        kind,
    })
}

fn build_function(
    resolve: &Resolve,
    root: PackageId,
    name: &str,
    function: &WitFunction,
) -> Result<Function> {
    match &function.kind {
        FunctionKind::Freestanding => {}
        FunctionKind::AsyncFreestanding
        | FunctionKind::Getter
        | FunctionKind::Setter
        | FunctionKind::Method(_)
        | FunctionKind::AsyncMethod(_)
        | FunctionKind::MethodGetter(_)
        | FunctionKind::MethodSetter(_)
        | FunctionKind::Static(_)
        | FunctionKind::AsyncStatic(_)
        | FunctionKind::StaticGetter(_)
        | FunctionKind::StaticSetter(_)
        | FunctionKind::Constructor(_) => {
            bail!("function `{name}` is not a freestanding function")
        }
    }
    let params = function
        .params
        .iter()
        .map(|param| {
            Ok(Param {
                name: param.name.clone(),
                ty: type_expr(resolve, root, &param.ty)
                    .with_context(|| format!("parameter `{name}({})`", param.name))?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let result = function
        .result
        .as_ref()
        .map(|ty| type_expr(resolve, root, ty))
        .transpose()
        .with_context(|| format!("function `{name}` result"))?;
    Ok(Function {
        name: name.to_string(),
        docs: docs(&function.docs),
        stability: stability(&function.stability),
        params,
        result,
    })
}

fn build_world(resolve: &Resolve, root: PackageId, name: &str, id: WorldId) -> Result<World> {
    let world = &resolve.worlds[id];
    Ok(World {
        name: name.to_string(),
        docs: docs(&world.docs),
        stability: stability(&world.stability),
        imports: build_world_side(resolve, root, id, "imports", world.imports.iter())?,
        exports: build_world_side(resolve, root, id, "exports", world.exports.iter())?,
    })
}

fn build_world_side<'a>(
    resolve: &Resolve,
    root: PackageId,
    world_id: WorldId,
    verb: &str,
    items: impl Iterator<Item = (&'a WorldKey, &'a WorldItem)>,
) -> Result<Vec<WorldInterface>> {
    let mut items: Vec<_> = items.collect();
    items.sort_by_key(|(_, item)| source_key(item.span()));
    let mut out = Vec::new();
    for (key, item) in items {
        match (key, item) {
            (WorldKey::Name(interface), WorldItem::Interface { id, .. }) => {
                if resolve.interfaces[*id].name.is_some() {
                    bail!("world {verb} named interface `{interface}`")
                }
                bail!("world {verb} inline interface `{interface}`")
            }
            (
                WorldKey::Interface(_),
                WorldItem::Interface {
                    id,
                    stability: item_stability,
                    docs: item_docs,
                    ..
                },
            ) => {
                let interface = &resolve.interfaces[*id];
                let name = interface
                    .name
                    .clone()
                    .context("world {verb} an inline interface")?;
                let package = match interface.package {
                    Some(package) if package != root => {
                        let wit_package = &resolve.packages[package];
                        let wit_version = wit_package
                            .name
                            .version
                            .as_ref()
                            .context("a dependency package has no version")?;
                        Some(Package {
                            namespace: wit_package.name.namespace.clone(),
                            name: wit_package.name.name.clone(),
                            version: Version {
                                major: wit_version.major,
                                minor: wit_version.minor,
                                patch: wit_version.patch,
                            },
                            docs: docs(&wit_package.docs),
                        })
                    }
                    Some(_) => None,
                    None => bail!("world {verb} an interface with no package"),
                };
                out.push(WorldInterface {
                    interface: name,
                    package,
                    docs: docs(item_docs),
                    stability: stability(item_stability),
                });
            }
            (_, WorldItem::Function(function)) => {
                bail!("world {verb} function `{}`", function.name)
            }
            (_, WorldItem::Type { id, .. }) => {
                let def = &resolve.types[*id];
                if def.owner == TypeOwner::World(world_id) {
                    if let WitTypeDefKind::Type(Type::Id(target)) = &def.kind {
                        // A world `use` names a type and defines nothing. Validate the target
                        // and do not add a world item.
                        type_expr(resolve, root, &Type::Id(*target))
                            .with_context(|| format!("world {verb} type use"))?;
                    } else {
                        bail!("world {verb} a type defined in the world");
                    }
                } else {
                    // A type copied in by `include`; it is not a world interface.
                    type_expr(resolve, root, &Type::Id(*id))
                        .with_context(|| format!("world {verb} included type"))?;
                }
            }
        }
    }
    Ok(out)
}

fn type_expr(resolve: &Resolve, root: PackageId, ty: &Type) -> Result<TypeExpr> {
    match ty {
        Type::Bool => Ok(TypeExpr::Bool),
        Type::U8 => Ok(TypeExpr::U8),
        Type::U16 => Ok(TypeExpr::U16),
        Type::U32 => Ok(TypeExpr::U32),
        Type::U64 => Ok(TypeExpr::U64),
        Type::S8 => Ok(TypeExpr::S8),
        Type::S16 => Ok(TypeExpr::S16),
        Type::S32 => Ok(TypeExpr::S32),
        Type::S64 => Ok(TypeExpr::S64),
        Type::F32 => Ok(TypeExpr::F32),
        Type::F64 => Ok(TypeExpr::F64),
        Type::String => Ok(TypeExpr::String),
        Type::Char => bail!("`char` is outside the supported subset"),
        Type::ErrorContext => bail!("`error-context` is outside the supported subset"),
        Type::Id(id) => type_id_expr(resolve, root, *id),
    }
}

fn type_id_expr(resolve: &Resolve, root: PackageId, id: TypeId) -> Result<TypeExpr> {
    let def = &resolve.types[id];
    if let Some(name) = &def.name {
        let (interface, package) = match def.owner {
            TypeOwner::Interface(interface_id) => {
                let interface = &resolve.interfaces[interface_id];
                (
                    interface
                        .name
                        .clone()
                        .context("named type has no interface name")?,
                    interface.package,
                )
            }
            TypeOwner::World(_) => bail!("type `{name}` is defined in a world"),
            TypeOwner::None => bail!("named type `{name}` has no owner"),
        };
        let interface = if package == Some(root) {
            interface
        } else {
            let package = package.context("named type has no package")?;
            let package = &resolve.packages[package].name;
            format!("{}:{}/{}", package.namespace, package.name, interface)
        };
        match &def.kind {
            WitTypeDefKind::Record(_)
            | WitTypeDefKind::Variant(_)
            | WitTypeDefKind::Enum(_)
            | WitTypeDefKind::Flags(_)
            | WitTypeDefKind::Type(_)
            | WitTypeDefKind::List(_)
            | WitTypeDefKind::Option(_)
            | WitTypeDefKind::Result(_) => {}
            WitTypeDefKind::Resource
            | WitTypeDefKind::Handle(_)
            | WitTypeDefKind::Tuple(_)
            | WitTypeDefKind::Map(..)
            | WitTypeDefKind::FixedLengthList(..)
            | WitTypeDefKind::Future(_)
            | WitTypeDefKind::Stream(_)
            | WitTypeDefKind::Unknown => {
                bail!("type `{name}` uses an unsupported WIT construct")
            }
        }
        Ok(TypeExpr::Named(QualifiedName {
            interface,
            name: name.clone(),
        }))
    } else {
        match &def.kind {
            WitTypeDefKind::Type(target) => type_expr(resolve, root, target),
            WitTypeDefKind::List(element) => {
                Ok(TypeExpr::List(Box::new(type_expr(resolve, root, element)?)))
            }
            WitTypeDefKind::Option(payload) => Ok(TypeExpr::Option(Box::new(type_expr(
                resolve, root, payload,
            )?))),
            WitTypeDefKind::Result(result) => Ok(TypeExpr::Result(result_expr(
                resolve,
                root,
                result.ok.as_ref(),
                result.err.as_ref(),
            )?)),
            WitTypeDefKind::Record(_)
            | WitTypeDefKind::Variant(_)
            | WitTypeDefKind::Enum(_)
            | WitTypeDefKind::Flags(_) => bail!("anonymous record, variant, enum, or flags type"),
            WitTypeDefKind::Resource
            | WitTypeDefKind::Handle(_)
            | WitTypeDefKind::Tuple(_)
            | WitTypeDefKind::Map(..)
            | WitTypeDefKind::FixedLengthList(..)
            | WitTypeDefKind::Future(_)
            | WitTypeDefKind::Stream(_)
            | WitTypeDefKind::Unknown => bail!("unsupported anonymous WIT construct"),
        }
    }
}

fn result_expr(
    resolve: &Resolve,
    root: PackageId,
    ok: Option<&Type>,
    err: Option<&Type>,
) -> Result<ResultExpr> {
    Ok(ResultExpr {
        ok: ok
            .map(|ty| type_expr(resolve, root, ty).map(Box::new))
            .transpose()?,
        err: err
            .map(|ty| type_expr(resolve, root, ty).map(Box::new))
            .transpose()?,
    })
}

fn docs(docs: &WitDocs) -> String {
    docs.contents.clone().unwrap_or_default()
}

fn stability(stability: &WitStability) -> Stability {
    match stability {
        WitStability::Unknown => Stability::Unknown,
        WitStability::Unstable {
            feature,
            deprecated: None,
        } => Stability::Unstable {
            feature: feature.clone(),
        },
        WitStability::Stable {
            since,
            deprecated: None,
        } => Stability::Stable {
            since: version(since.major, since.minor, since.patch),
        },
        WitStability::Unstable {
            feature,
            deprecated: Some(deprecated),
        } => deprecated_stability(
            None,
            Some(feature.clone()),
            deprecated.major,
            deprecated.minor,
            deprecated.patch,
        ),
        WitStability::Stable {
            since,
            deprecated: Some(deprecated),
        } => deprecated_stability(
            Some(version(since.major, since.minor, since.patch)),
            None,
            deprecated.major,
            deprecated.minor,
            deprecated.patch,
        ),
    }
}

fn deprecated_stability(
    since: Option<Version>,
    feature: Option<String>,
    major: u64,
    minor: u64,
    patch: u64,
) -> Stability {
    let deprecated = version(major, minor, patch);
    Stability::Deprecated {
        since,
        feature,
        deprecated,
        note: format!("Deprecated in WIT version {deprecated}."),
    }
}

const fn version(major: u64, minor: u64, patch: u64) -> Version {
    Version {
        major,
        minor,
        patch,
    }
}

fn source_key(span: Span) -> (u8, u32) {
    if span.is_known() {
        (0, span.start())
    } else {
        (1, 0)
    }
}

/// The kind of a numbered lockfile member.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum MemberKind {
    /// A record field.
    Field,
    /// A variant case.
    VariantCase,
    /// An enum case.
    EnumCase,
    /// A flags member.
    Flag,
    /// A function method id.
    Method,
    /// A function parameter.
    Param,
}

impl MemberKind {
    /// The lockfile token for this kind.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Field => "field",
            Self::VariantCase => "variant-case",
            Self::EnumCase => "enum-case",
            Self::Flag => "flag",
            Self::Method => "method",
            Self::Param => "param",
        }
    }
}

impl fmt::Display for MemberKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for MemberKind {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "field" => Ok(Self::Field),
            "variant-case" => Ok(Self::VariantCase),
            "enum-case" => Ok(Self::EnumCase),
            "flag" => Ok(Self::Flag),
            "method" => Ok(Self::Method),
            "param" => Ok(Self::Param),
            _ => Err("unknown member kind"),
        }
    }
}

/// One numbered lockfile member: a field, case, flag, method id, or parameter.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[non_exhaustive]
pub struct Member {
    /// The parent whose number space the member belongs to.
    ///
    /// For a type member this is `namespace:package/interface/type`; for a method it is
    /// `namespace:package/interface`; for a parameter it is
    /// `namespace:package/interface#function`.
    pub parent: String,
    /// The member's name within its parent.
    pub name: String,
    /// The member kind.
    pub kind: MemberKind,
    /// The canonical type, when the member has one.
    ///
    /// Fields and parameters require one; variant cases and methods have an optional type;
    /// enum cases and flags have none.
    pub ty: Option<String>,
}

impl Member {
    /// Create a numbered lockfile member.
    #[must_use]
    pub fn new(parent: String, name: String, kind: MemberKind, ty: Option<String>) -> Self {
        Self {
            parent,
            name,
            kind,
            ty,
        }
    }

    /// The lockfile key, `parent.name`.
    ///
    /// The package version is deliberately not part of the key, so a version bump never
    /// renumbers anything.
    #[must_use]
    pub fn key(&self) -> String {
        format!("{}.{}", self.parent, self.name)
    }
}

impl Model {
    /// Every numbered member of the model, in deterministic source order.
    ///
    /// Members are emitted for interfaces, their types in source order, the members within
    /// each type in source order, then functions and their parameters in source order. Aliases,
    /// named `list`/`option`/`result` definitions, interfaces, and worlds produce no members.
    ///
    /// # Errors
    ///
    /// Returns an error if a member's type cannot be rendered canonically.
    pub fn members(&self) -> Result<Vec<Member>> {
        let package = format!("{}:{}", self.package.namespace, self.package.name);
        let mut out = Vec::new();
        for interface in &self.interfaces {
            for def in &interface.types {
                let parent = format!("{package}/{}/{}", interface.name, def.name);
                match &def.kind {
                    TypeDefKind::Record(fields) => {
                        for field in fields {
                            out.push(Member {
                                parent: parent.clone(),
                                name: field.name.clone(),
                                kind: MemberKind::Field,
                                ty: Some(self.canonical_type(&field.ty).with_context(|| {
                                    format!("member `{parent}.{}`", field.name)
                                })?),
                            });
                        }
                    }
                    TypeDefKind::Variant(cases) => {
                        for case in cases {
                            out.push(Member {
                                parent: parent.clone(),
                                name: case.name.clone(),
                                kind: MemberKind::VariantCase,
                                ty: case
                                    .payload
                                    .as_ref()
                                    .map(|ty| self.canonical_type(ty))
                                    .transpose()
                                    .with_context(|| format!("member `{parent}.{}`", case.name))?,
                            });
                        }
                    }
                    TypeDefKind::Enum(cases) => {
                        out.extend(cases.iter().map(|case| Member {
                            parent: parent.clone(),
                            name: case.name.clone(),
                            kind: MemberKind::EnumCase,
                            ty: None,
                        }));
                    }
                    TypeDefKind::Flags(flags) => {
                        out.extend(flags.iter().map(|flag| Member {
                            parent: parent.clone(),
                            name: flag.name.clone(),
                            kind: MemberKind::Flag,
                            ty: None,
                        }));
                    }
                    TypeDefKind::Alias(_)
                    | TypeDefKind::List(_)
                    | TypeDefKind::Option(_)
                    | TypeDefKind::Result(_) => {}
                }
            }

            for function in &interface.functions {
                let parent = format!("{package}/{}", interface.name);
                let method_key = format!("{parent}.{}", function.name);
                out.push(Member {
                    parent: parent.clone(),
                    name: function.name.clone(),
                    kind: MemberKind::Method,
                    ty: function
                        .result
                        .as_ref()
                        .map(|ty| self.canonical_type(ty))
                        .transpose()
                        .with_context(|| format!("member `{method_key}`"))?,
                });

                let param_parent = format!("{parent}#{}", function.name);
                for param in &function.params {
                    out.push(Member {
                        parent: param_parent.clone(),
                        name: param.name.clone(),
                        kind: MemberKind::Param,
                        ty: Some(
                            self.canonical_type(&param.ty).with_context(|| {
                                format!("member `{param_parent}.{}`", param.name)
                            })?,
                        ),
                    });
                }
            }
        }
        Ok(out)
    }
}

/// Build the model and return its numbered members.
///
/// This is a convenience wrapper around [`Model::members`].
///
/// # Errors
///
/// Returns an error if the contract is outside the subset, cannot be modelled, or a member's
/// type cannot be rendered canonically.
pub fn members(contract: &Contract) -> Result<Vec<Member>> {
    build(contract)?.members()
}
