# witforge internal model

Status: specified 2026-10-25; implementation pending. This document is normative for the model
that sits between `wit-parser` and the lockfile/emitters. The lockfile format is in
[`lockfile.md`](lockfile.md), the generated-code conventions are in [`emitters.md`](emitters.md),
and the decisions are logged in [`overview.md`](overview.md#9-decision-log).

The key words MUST, MUST NOT, SHOULD, and MAY are used as in RFC 2119.

## 1. Purpose

`wit-parser` makes breaking 0.x releases roughly monthly and exposes exhaustive enums plus
arena IDs. witforge's public API MUST NOT expose those types. The internal model is a small,
owned, read-only representation of one contract version:

- it is built once from a resolved contract and then used by the lockfile and emitters;
- it carries types, docs, stability, interfaces, functions, worlds, and source order;
- it is independent of `wit-parser` types, so an upstream bump changes one conversion module;
- it is total over the supported subset. Anything outside the subset is refused before model
  construction, so the model never has to represent resources, tuples, maps, `future`, `stream`,
  `error-context`, foreign types, or world-defined types/functions.

The model does not resolve names at emit time. Named references are by qualified name, and the
canonical type rules are shared with the lockfile.

## 2. Shape

The model is conceptually:

```rust,ignore
pub struct Model {
    pub package: Package,
    pub interfaces: Vec<Interface>,   // source order
    pub worlds: Vec<World>,           // source order
}

pub struct Package {
    pub namespace: String,
    pub name: String,
    pub version: Version,             // major.minor.0 only
    pub docs: String,
}

pub struct Interface {
    pub name: String,
    pub docs: String,
    pub stability: Stability,
    pub types: Vec<TypeDef>,          // source order
    pub functions: Vec<Function>,     // source order
}

pub struct TypeDef {
    pub name: String,
    pub docs: String,
    pub stability: Stability,
    pub kind: TypeDefKind,
}

pub enum TypeDefKind {
    Record(Vec<Field>),
    Variant(Vec<Case>),
    Enum(Vec<Case>),
    Flags(Vec<Flag>),
    Alias(TypeExpr),                  // `type a = b` and `use` aliases
    List(TypeExpr),                   // named `type x = list<T>`
    Option(TypeExpr),                 // named `type x = option<T>`
    Result(ResultExpr),               // named `type x = result<T, E>`
}

pub struct Field {
    pub name: String,
    pub docs: String,
    pub ty: TypeExpr,
}

pub struct Case {
    pub name: String,
    pub docs: String,
    pub payload: Option<TypeExpr>,    // None for a payload-less case
}

pub struct Flag {
    pub name: String,
    pub docs: String,
}

pub struct Function {
    pub name: String,                 // unmangled item name
    pub docs: String,
    pub stability: Stability,
    pub params: Vec<Param>,           // source order
    pub result: Option<TypeExpr>,     // None for no result
}

pub struct Param {
    pub name: String,
    pub ty: TypeExpr,
}

pub struct World {
    pub name: String,
    pub docs: String,
    pub stability: Stability,
    pub imports: Vec<WorldInterface>,
    pub exports: Vec<WorldInterface>,
}

pub struct WorldInterface {
    pub interface: String,            // interface name in this package
    pub docs: String,
    pub stability: Stability,
}

pub enum TypeExpr {
    Bool,
    U8, U16, U32, U64,
    S8, S16, S32, S64,
    F32, F64,
    String,
    Named(QualifiedName),
    List(Box<TypeExpr>),
    Option(Box<TypeExpr>),
    Result(ResultExpr),
}

pub struct ResultExpr {
    pub ok: Option<Box<TypeExpr>>,    // None means `_`
    pub err: Option<Box<TypeExpr>>,   // None means `_`
}

pub struct QualifiedName {
    pub interface: String,
    pub name: String,
}

pub enum Stability {
    Unknown,
    Unstable { feature: String },
    Stable { since: Version },
}
```

`Version` is a small owned `major/minor/patch` value with `Display`; witforge does not add a
`semver` dependency for the model. Contract versions are restricted to `major.minor.0`.

Implementation may use `Box`/`Vec`/`Option` or compact indices; the public semantics above are
normative. Names are WIT names with `%` escaping removed.

## 3. Construction and invariants

`load` resolves the package and returns an opaque `Contract` whose `wit-parser` fields are
crate-private. The model is then built from that contract:

```text
load(dir) -> Contract (crate-private wit-parser state)
subset::check(&Contract) -> Violations
model::build(&Contract) -> Result<Model>
```

- `model::build` MUST be the only module that converts `wit-parser` types into model types.
- It MUST return an error for a type or function the subset should have refused, rather than
  silently dropping it.
- It MUST preserve source order of interfaces, types, functions, members, and parameters, as
  observed in `wit-parser`'s `IndexMap`s.
- It MUST use the unmangled item name for functions, not the `[method]`-style name.
- It MUST include every interface and world in the root package, including unused ones.
- It MUST include `@unstable` items; all features are loaded.
- It MUST copy docs and stability from `wit-parser`. Record fields, variant cases, enum cases,
  and flags have no WIT stability attribute; their stability is the owning type's stability.
- It MUST keep `type a = b` and `use` aliases as `TypeDefKind::Alias`; aliases create no lockfile
  members and are expanded for canonical types. Rust emits them as aliases; proto inlines them.
- It MUST keep named `list`, `option`, and `result` types as their structural kinds (not as
  aliases); the Rust emitter emits an alias, the proto emitter inlines them, and the lockfile
  expands them at use sites.
- A named reference MUST resolve to the defining interface. A reference to a type in a world,
  another package, or an interface outside the package is an error.
- `TypeExpr::Named` preserves the name written at the reference site, including a `use` alias or
  a named list/option/result definition. Rust uses that name (and emits an alias where needed);
  canonical display expands it as described in section 4; proto inlines it.
- Worlds MUST contain only the named interfaces they import or export. A world `use` names a
  type and defines nothing; it is not a world item in the model. World-defined types/functions
  are refused before construction.
- The package version MUST be `major.minor.0`; the model uses the version for module/proto names
  and freeze checks, but never for lockfile keys.
- The model MUST NOT contain source spans in 0.1. If diagnostics need them later, add them as an
  additive field.

## 4. Canonical types

`TypeExpr` has one canonical display form, used by lockfile entries and comparisons:

- primitives use WIT spelling (`bool`, `u8`, `s64`, `f64`, `string`);
- a named record/variant/enum/flags is `interface/type` (package version omitted);
- an alias expands recursively to its target;
- a named `list`/`option`/`result` expands structurally;
- `list<T>` is `list<T>`, `option<T>` is `option<T>`;
- `result` is `result<ok,err>` with both slots and `_` for a missing payload;
- no whitespace.

The full grammar and examples are in [`lockfile.md`](lockfile.md#5-canonical-types). The model
MUST expose one method that renders a `TypeExpr` canonically; the lockfile MUST use it rather
than reimplementing the rules.

## 5. Member extraction

The model exposes deterministic members for the lockfile:

- record field: kind `field`, type required, parent `interface/type`;
- variant case: kind `variant-case`, type present for a payload and absent otherwise, parent
  `interface/type`;
- enum case: kind `enum-case`, no type, parent `interface/type`;
- flag: kind `flag`, no type, parent `interface/type`;
- function: kind `method`, parent `interface`, key `interface.function`, type is the result type
  or absent for no result;
- parameter: kind `param`, parent `interface#function`, key
  `interface#function.parameter`, type required.

Aliases, named list/option/result types, interfaces, and worlds produce no members. A member's
key is exactly the lockfile key in [`lockfile.md`](lockfile.md#3-keys). Members are produced in
source order: interfaces, types within an interface, members within a type, then functions and
their parameters; this order determines number assignment among simultaneously new members in
one parent. The lockfile renders sorted by key.

## 6. Public API

The public model API SHOULD be:

```rust,ignore
pub fn build(contract: &Contract) -> anyhow::Result<Model>;

impl Model {
    pub fn interface(&self, name: &str) -> Option<&Interface>;
    pub fn type_def(&self, name: &QualifiedName) -> Option<&TypeDef>;
}
```

`Contract` itself stays opaque: its `wit-parser` fields are crate-private. `Model` and its
component types are public, owned, and `Debug + Clone + PartialEq`. A `TypeExpr` exposes its
canonical display through `Display` or an equivalent method.

Typed error reporting is a later API-stability question. For 0.1, `anyhow` is acceptable at the
boundary, but internal conversion errors SHOULD name the item and the unsupported construct.

## 7. Testing requirements

- one fixture per supported construct: each primitive, record, variant with and without
  payload, enum, flags, list, option, named list/option/result, alias, `use` alias,
  one-sided/bare `result`, function with/without result, and each parameter kind;
- docs and stability on interfaces, types, functions, and worlds;
- `@unstable` items retained and `@since` carried;
- escaped/keyword identifiers (`%type`, `%where`, `%self`) retained as raw names;
- source order preserved for interfaces, types, members, functions, and parameters;
- canonical display matches golden strings for every construct and nesting;
- member extraction matches golden keys/kinds/types and skips aliases/named structural types;
- an interface-local `use ... as ...` alias resolves to the defining type;
- refused constructs and foreign/world-owned types return a model error or are stopped by the
  subset before construction;
- a compile-time or API test that no `wit_parser` type appears in the public model API.

## 8. Deferred and pre-golive

- Source spans and structured diagnostics.
- Deprecation attributes and doc examples.
- Stability-aware generation/gating; 0.1 treats stability as metadata.
- World role metadata in generated code (see the pre-golive world task).
- A stable public model API and typed errors before 1.0.
