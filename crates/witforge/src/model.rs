//! The owned internal model of one contract version.
//!
//! The model is the boundary between `wit-parser` and the rest of witforge. It contains no
//! `wit-parser` types, so an upstream bump only changes the conversion code in [`build`]. The
//! subset checker runs before construction; the conversion also refuses anything it cannot
//! represent, such as resources, tuples, foreign types, and world-defined types.

use std::collections::HashSet;
use std::fmt;

use anyhow::{Context, Result, bail, ensure};
use wit_parser::{
    Docs as WitDocs, Function as WitFunction, FunctionKind, InterfaceId, PackageId, Resolve,
    Stability as WitStability, Type, TypeDefKind as WitTypeDefKind, TypeId, TypeOwner, WorldId,
    WorldItem, WorldKey,
};

use crate::Contract;

/// A contract version, used for package versions and `@since` values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version {
    /// Major version.
    pub major: u64,
    /// Minor version.
    pub minor: u64,
    /// Patch version.
    pub patch: u64,
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// WIT stability metadata attached to an item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stability {
    /// No `@since` or `@unstable` attribute.
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
}

/// A type reference to a named type in the contract package.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
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

/// A type expression in the model.
#[derive(Debug, Clone, PartialEq, Eq)]
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
pub struct ResultExpr {
    /// The success payload, or `None` for `_`.
    pub ok: Option<Box<TypeExpr>>,
    /// The error payload, or `None` for `_`.
    pub err: Option<Box<TypeExpr>>,
}

/// A record field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    /// The field name.
    pub name: String,
    /// Documentation from WIT.
    pub docs: String,
    /// The field type.
    pub ty: TypeExpr,
}

/// A variant or enum case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Case {
    /// The case name.
    pub name: String,
    /// Documentation from WIT.
    pub docs: String,
    /// The variant payload; `None` for a payload-less case or an enum case.
    pub payload: Option<TypeExpr>,
}

/// A flags member.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Flag {
    /// The flag name.
    pub name: String,
    /// Documentation from WIT.
    pub docs: String,
}

/// The definition of a named type in an interface.
#[derive(Debug, Clone, PartialEq, Eq)]
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

/// The shape of a named type.
#[derive(Debug, Clone, PartialEq, Eq)]
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
pub struct Param {
    /// The parameter name.
    pub name: String,
    /// The parameter type.
    pub ty: TypeExpr,
}

/// An interface function.
#[derive(Debug, Clone, PartialEq, Eq)]
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

/// A named interface and its definitions.
#[derive(Debug, Clone, PartialEq, Eq)]
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

/// A named interface imported or exported by a world.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldInterface {
    /// The interface name in this package.
    pub interface: String,
    /// Documentation on the import or export statement.
    pub docs: String,
    /// Stability metadata on the import or export statement.
    pub stability: Stability,
}

/// A world and its imported and exported interfaces.
#[derive(Debug, Clone, PartialEq, Eq)]
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

/// The contract package metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
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

/// The owned model of one contract version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Model {
    /// The package metadata.
    pub package: Package,
    /// Interfaces in source order.
    pub interfaces: Vec<Interface>,
    /// Worlds in source order.
    pub worlds: Vec<World>,
}

impl Model {
    /// Find an interface by name.
    #[must_use]
    pub fn interface(&self, name: &str) -> Option<&Interface> {
        self.interfaces
            .iter()
            .find(|interface| interface.name == name)
    }

    /// Find a named type by its qualified name.
    #[must_use]
    pub fn type_def(&self, name: &QualifiedName) -> Option<&TypeDef> {
        self.interface(&name.interface)?
            .types
            .iter()
            .find(|def| def.name == name.name)
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
        self.canonical_type_inner(ty, &mut HashSet::new())
    }

    fn canonical_type_inner(
        &self,
        ty: &TypeExpr,
        visiting: &mut HashSet<QualifiedName>,
    ) -> Result<String> {
        match ty {
            TypeExpr::Bool => Ok("bool".to_string()),
            TypeExpr::U8 => Ok("u8".to_string()),
            TypeExpr::U16 => Ok("u16".to_string()),
            TypeExpr::U32 => Ok("u32".to_string()),
            TypeExpr::U64 => Ok("u64".to_string()),
            TypeExpr::S8 => Ok("s8".to_string()),
            TypeExpr::S16 => Ok("s16".to_string()),
            TypeExpr::S32 => Ok("s32".to_string()),
            TypeExpr::S64 => Ok("s64".to_string()),
            TypeExpr::F32 => Ok("f32".to_string()),
            TypeExpr::F64 => Ok("f64".to_string()),
            TypeExpr::String => Ok("string".to_string()),
            TypeExpr::Named(name) => self.canonical_named(name, visiting),
            TypeExpr::List(element) => Ok(format!(
                "list<{}>",
                self.canonical_type_inner(element, visiting)?
            )),
            TypeExpr::Option(payload) => Ok(format!(
                "option<{}>",
                self.canonical_type_inner(payload, visiting)?
            )),
            TypeExpr::Result(result) => self.canonical_result(result, visiting),
        }
    }

    fn canonical_result(
        &self,
        result: &ResultExpr,
        visiting: &mut HashSet<QualifiedName>,
    ) -> Result<String> {
        let ok = match &result.ok {
            Some(ty) => self.canonical_type_inner(ty, visiting)?,
            None => "_".to_string(),
        };
        let err = match &result.err {
            Some(ty) => self.canonical_type_inner(ty, visiting)?,
            None => "_".to_string(),
        };
        Ok(format!("result<{ok},{err}>"))
    }

    fn canonical_named(
        &self,
        name: &QualifiedName,
        visiting: &mut HashSet<QualifiedName>,
    ) -> Result<String> {
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
            }) => self.canonical_type_inner(target, visiting),
            Some(TypeDef {
                kind: TypeDefKind::List(element),
                ..
            }) => Ok(format!(
                "list<{}>",
                self.canonical_type_inner(element, visiting)?
            )),
            Some(TypeDef {
                kind: TypeDefKind::Option(payload),
                ..
            }) => Ok(format!(
                "option<{}>",
                self.canonical_type_inner(payload, visiting)?
            )),
            Some(TypeDef {
                kind: TypeDefKind::Result(result),
                ..
            }) => self.canonical_result(result, visiting),
            None => bail!("type `{name}` is not in the model"),
        };
        visiting.remove(name);
        result
    }
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

/// Build the owned model from a loaded contract.
///
/// The subset checker is run first. A contract outside the subset is an error rather than a
/// partially populated model, and a construct this conversion does not recognise is also an
/// error rather than silently dropped.
///
/// # Errors
///
/// Returns an error for a subset violation, an unsupported `wit-parser` construct, a foreign or
/// world-defined type, a non-freestanding function, or a contract version that is not
/// `major.minor.0`.
pub fn build(contract: &Contract) -> Result<Model> {
    let violations = crate::subset::check(contract);
    if !violations.is_empty() {
        let messages: Vec<String> = violations.iter().map(ToString::to_string).collect();
        bail!(
            "contract does not satisfy the subset: {}",
            messages.join("; ")
        );
    }

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

    let interfaces = wit_package
        .interfaces
        .iter()
        .map(|(name, id)| build_interface(resolve, root, name, *id))
        .collect::<Result<Vec<_>>>()?;
    let worlds = wit_package
        .worlds
        .iter()
        .map(|(name, id)| build_world(resolve, root, name, *id))
        .collect::<Result<Vec<_>>>()?;

    Ok(Model {
        package,
        interfaces,
        worlds,
    })
}

fn build_interface(
    resolve: &Resolve,
    root: PackageId,
    name: &str,
    id: InterfaceId,
) -> Result<Interface> {
    let interface = &resolve.interfaces[id];
    let types = interface
        .types
        .iter()
        .map(|(type_name, type_id)| build_type_def(resolve, root, type_name, *type_id))
        .collect::<Result<Vec<_>>>()?;
    let functions = interface
        .functions
        .iter()
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
    let mut out = Vec::new();
    for (key, item) in items {
        match (key, item) {
            (WorldKey::Name(interface), WorldItem::Interface { .. }) => {
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
                if interface.package != Some(root) {
                    bail!("world {verb} an interface from another package");
                }
                let name = interface
                    .name
                    .clone()
                    .context("world {verb} an inline interface")?;
                out.push(WorldInterface {
                    interface: name,
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
        ensure!(
            package == Some(root),
            "type `{name}` comes from another package"
        );
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
        WitStability::Unstable { feature, .. } => Stability::Unstable {
            feature: feature.clone(),
        },
        WitStability::Stable { since, .. } => Stability::Stable {
            since: Version {
                major: since.major,
                minor: since.minor,
                patch: since.patch,
            },
        },
    }
}

/// The kind of a numbered lockfile member.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
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
