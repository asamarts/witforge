# witforge internal model

Status: specified 2026-10-25; implemented 2026-10-28. This document is normative for the model
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
  `error-context`, or world-defined types/functions. Named types and world interfaces from
  `wit/deps` dependencies are retained as package-qualified references; see
  [`dependencies.md`](dependencies.md).

The model does not resolve names at emit time. Named references are by qualified name, and the
canonical type rules are shared with the lockfile.

## 2. Shape

The model is conceptually:

```rust,ignore
pub struct Model {
    pub package: Package,
    pub interfaces: Vec<Interface>,   // source order
    pub worlds: Vec<World>,           // source order
    pub dependencies: Vec<Dependency>, // standard deps, sorted by package name
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
    pub interface: String,            // interface name within its package
    pub package: Option<Package>,     // the dependency package, or None for the root
    pub docs: String,
    pub stability: Stability,
}

pub struct Dependency {
    pub package: Package,
    pub interfaces: Vec<Interface>,
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
    // Local: `interface/type`. Dependency: `namespace:package/interface/type`.
    pub interface: String,
    pub name: String,
}

pub enum Stability {
    Unknown,
    Unstable { feature: String },
    Stable { since: Version },
    Deprecated {
        since: Option<Version>,
        feature: Option<String>,
        deprecated: Version,
        note: String,
    },
}
```

`Version` is a small owned `major/minor/patch` value with `Display`; witforge does not add a
`semver` dependency for the model. Contract versions are restricted to `major.minor.0`.

Implementation may use `Box`/`Vec`/`Option` or compact indices; the public semantics above are
normative. Names are WIT names with `%` escaping removed. Requiredness is structural: a field or
parameter value is optional only when its `TypeExpr` is `Option`; a function result is absent
(`None`) or carries a type whose optionality follows that type. The proto and glue emitters use
this distinction for presence checks.

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
- It MUST preserve source order of interfaces, types, functions, members, and parameters. For a
  multi-file package (both the root version directory and each package directory under `deps/`),
  `*.wit` files are loaded in ascending **file name byte order** (UTF-8 bytes, never locale or OS
  directory-enumeration order), and items within each file keep their source position order.
  Dependency entries under `deps/` are loaded by the same byte-order rule. Renaming a file in a
  multi-file package therefore changes declaration order deterministically. Use ASCII file names
  so case-insensitive or Unicode-normalising file systems cannot alter file-name bytes.
- It MUST use the unmangled item name for functions, not the `[method]`-style name.
- It MUST include every interface and world in the root package, including unused ones.
- It MUST include `@unstable` items; all features are loaded.
- It MUST copy docs and stability from `wit-parser`, including the introduction or feature that
  accompanies `@deprecated`. Because WIT's `@deprecated(version = ...)` has no note operand, the
  model synthesizes the stable note `Deprecated in WIT version <version>.`. Record fields,
  variant cases, enum cases, and flags have no WIT stability attribute; their stability is the
  owning type's stability and a single member cannot be deprecated independently.
- It MUST keep `type a = b` and `use` aliases as `TypeDefKind::Alias`; aliases create no lockfile
  members and are expanded for canonical types. Rust emits them as aliases; proto inlines them.
- It MUST keep named `list`, `option`, and `result` types as their structural kinds (not as
  aliases). The Rust emitter emits an alias; the proto emitter applies the structural
  projection, using a named message when that projection requires one (a named result, or an
  option of a list/option), as specified in [`emitters.md`](emitters.md); the lockfile expands
  them at use sites.
- A named reference MUST resolve to the defining interface. A local reference resolves in the
  root package; a reference with package identity resolves in the corresponding standard
  `deps/` package. A reference to a type defined in a world, or to a package outside the
  standard dependency layout, is an error.
- `TypeExpr::Named` preserves the name written at the reference site, including a `use` alias or
  a named list/option/result definition. Rust uses that name (and emits an alias where needed);
  canonical display expands it as described in section 4; proto inlines it.
- Worlds MUST contain only the named interfaces they import or export directly. Named interface
  imports and exports (`import foo: types;`, `export bar: types;`), inline interfaces
  (`import foo: interface { ... }`), world-defined types, and world-level functions are refused
  before construction, each with its own diagnostic. A world `use` names a type and defines
  nothing; it is not a world item in the model. An imported or exported interface may belong to
  the root package or to a standard `deps/` package; its package identity is preserved. When a
  world reaches a dependency interface (explicitly or through `wit-parser`'s implicit import of a
  dependency used by a world item), every item in that dependency interface becomes required, and
  diagnostics for unsupported items there name the world item that pulled it in.
- Only `@since`, `@unstable`, and `@deprecated` annotations are accepted. `wit-parser` requires
  `@deprecated(version = ...)` beside either `@since` or `@unstable`; deprecation by itself,
  both introduction forms together, duplicate annotations, `@external-id`, and unknown
  annotations are refused.
- Every annotation version MUST be `<major>.<minor>.0` and no later than its containing package
  version; a deprecation version MUST NOT predate its `@since` version. These are contract
  violations when the syntax reaches the subset checker; version gates rejected by `wit-parser`
  remain parse errors.
- A WIT-root check and every generator compare `@since` against declaration history. When the
  root contains the exact claimed version, the item MUST first appear in that directory. If the
  claimed version is absent, a partial history cannot prove the introduction point and does not
  reject it. A single-version `check` cannot perform this cross-version rule.
- Names within one scope (interfaces and worlds of a package, types and functions of an interface,
  and live or reserved members of a lockfile parent) MUST be distinct ASCII case-insensitively;
  names in different scopes do not collide.
- The package version and every dependency package version under `deps/` (referenced or not) MUST
  be `major.minor.0`; violations are reported as subset diagnostics (exit `1`) naming the
  package, its version, and its `deps/` entry. The model uses the version for module/proto names
  and freeze checks, but never for lockfile keys.
- Unsupported items in dependency packages that no root type or world reaches are skipped
  during model construction, together with every dependency type or function that transitively
  references them (within the same dependency package or across dependency packages). Pruning
  runs to a fixed point so no emitter ever sees a dangling dependency reference or produces a
  dangling Rust alias or JSON Schema `$ref`.
- `subset::check_diagnostics` validates every single-version contract refusal that any generator
  could make—including Rust/glue/proto/prost name collisions, reserved generated names
  (`functions`, `witforge_value`, `UNSPECIFIED`, `Empty`), dependency module/filename collisions,
  synthesized wrapper collisions and byte limits, and section 4.1 expansion limits—so a passing
  single-version `check` implies that `rust`, `proto`, `schema`, and `glue` all succeed.
- The model MUST NOT contain source spans in the current 0.x line. If diagnostics need them later, add them as an
  additive field.

## 4. Canonical types

`TypeExpr` has one canonical display form, used by lockfile entries and comparisons:

- primitives use WIT spelling (`bool`, `u8`, `s64`, `f64`, `string`);
- a named record/variant/enum/flags is `interface/type` (package version omitted);
- a local alias expands recursively to its target;
- a local named `list`/`option`/`result` expands structurally;
- a named type imported from a dependency—alias, named structural, or member-bearing—is written
  as its package-qualified name (`ex:shared/types/id`, `ex:shared/types/items`) and is not
  expanded; the dependency version is not part of the name;
- `list<T>` is `list<T>`, `option<T>` is `option<T>`;
- `result` is `result<ok,err>` with both slots and `_` for a missing payload;
- no whitespace.

The full grammar and examples are in [`lockfile.md`](lockfile.md#5-canonical-types). The model
MUST expose one method that renders a `TypeExpr` canonically; the lockfile MUST use it rather
than reimplementing the rules.

### 4.1 Expansion limits

Every model is checked once, after construction and before any lockfile or emitter walks it:

- at most **32** nested `list`/`option`/`result` levels in one type expression;
- at most **32** alias links followed in one chain;
- at most **1024 bytes** in one canonical type string.

The limits are separate: a chain of aliases to `u8` consumes the alias limit but no nesting, and
the doubling `type tN = list<result<tN-1, tN-1>>` consumes nesting and output length. A
canonical type over the byte limit is refused while it is being built, so the output is never
allocated. The same limits apply to every definition and function in the root package and every
`deps/` package, including definitions nothing references; the lockfile reader applies the
nesting and byte limits to a type token and refuses an over-limit token as a parse error.

The limit diagnostic is a contract violation (exit `1`) when it comes from a contract and a
parse error (exit `2`) when it comes from the lockfile. `wit-parser` also has its own source
nesting bound; a WIT source nested beyond it is refused during `load` with exit `2` before the
model exists. Raising a limit later is compatible; lowering one is not.

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

Typed error reporting is a later API-stability question. For the current 0.x line, `anyhow` is acceptable at the
boundary, but internal conversion errors SHOULD name the item and the unsupported construct.

## 7. Testing requirements

- one fixture per supported construct: each primitive, record, variant with and without
  payload, enum, flags, list, option, named list/option/result, alias, `use` alias,
  one-sided/bare `result`, function with/without result, and each parameter kind;
- docs and stability on interfaces, types, functions, worlds, and world imports/exports;
- stable and unstable deprecation forms, the synthesized note, invalid annotation versions, and
  the parser requirement that deprecation accompany `@since` or `@unstable`;
- `@unstable` items retained, `@since` carried, and root history validating the first declaration;
- source order preserved for interfaces, types, members, functions, and parameters;
- canonical display matches golden strings for every construct and nesting;
- member extraction matches golden keys/kinds/types and skips aliases/named structural types;
- an interface-local `use ... as ...` alias resolves to the defining type;
- a `use` alias to a named type in a standard `deps/` package retains `namespace:package`
  identity;
- refused constructs and world-owned types return a model error or are stopped by the subset
  before construction;
- a compile-time or API test that no `wit_parser` type appears in the public model API.

## 8. Deferred and pre-golive

- Source spans in the public model. Structured diagnostics retain positions outside the model.
- Resource, handle, and other post-1.0 subset expansion follows the public-API policy in
  [`public-api.md`](public-api.md).
- Typed errors are not promised for 1.0: the boundary stays `anyhow::Error` with
  `ContractError` classification.
