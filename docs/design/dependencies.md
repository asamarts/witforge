# WIT package dependencies

Status: implemented 2026-11-06 (`W9-001`, `W9-004`, `W9-005`); one-version-per-graph and
root-name refusal 2026-11-19 (`W10-047`).

witforge accepts the standard WIT dependency layout: a version directory may contain a `deps/`
directory of WIT packages, and a local package may reference a named type from a dependency with
the normal WIT `use` syntax, including its package version:

```wit
package example:app@0.1.0;

interface api {
    use example:shared/types@0.1.0.{item};
    get: func() -> item;
}
```

```text
wit/v0.1/
  contract.wit
  deps/shared/types.wit
  wire.lock
```

The dependency package declares `package example:shared@0.1.0;`. The version is part of the
WIT dependency pin; `wit-parser` resolves it before the model is built. Missing, duplicate, or
unresolvable dependency packages fail `load` with exit `2` rather than being guessed.

## Package identity

A dependency type is represented as a package-qualified `QualifiedName`:

```text
example:shared/types/item
 ^namespace ^package ^interface ^type
```

Local names keep the existing `interface/type` form. A foreign world import or export uses the
same package identity:

```wit
world plugin {
    import example:shared/types@0.1.0;
    export example:other/types@0.1.0;
}
```

The model stores the interface name and the dependency package metadata separately; its
qualified name is `example:shared/types`. The dependency version is not part of a lockfile key
or canonical type: the dependency pin is the versioned `use` path plus the content under
`deps/`, and a dependency version bump does not renumber local members. Within one major, the
cross-version rule below still refuses a dependency change; across majors, a new major may change,
add, remove, or upgrade dependencies.

## Generated references

The emitters keep dependency references external rather than copying or redefining another
package's types:

- Rust: `super::super::witforge_deps::<namespace>_<package>::<interface>::<Type>`.
  Inside a dependency's own generated module, a reference to another dependency is
  `super::super::<namespace>_<package>::<interface>::<Type>`.
- Protobuf: `.namespace.name.v<major>.<Interface><Type>` (with a leading dot so innermost-first
  protobuf name resolution cannot shadow a dependency package when a root package shares a
  segment, and `-` replaced by `_` in package and file names). Intra-package references stay
  relative.

The Rust emitter writes dependency modules into `witforge_deps.rs`; the protobuf emitter writes
one `<namespace>.<package>.v<major>.proto` file per dependency and emits sorted `import` lines
only for dependency schemas that an emitted message, field, oneof, wrapper, request, or response
actually references (dependencies reached only through a world import/export or through an
inlined scalar/byte alias emit their own `.proto` file without an unused `import`). World metadata uses the package-qualified interface name. A dependency
may use a type from another package under the same `deps/` directory; that import is emitted
from the dependency's schema as well. Alias, list, option, and result shapes resolve through
their dependency definitions, so a foreign alias maps to its underlying message or scalar
rather than to a non-existent alias message. A foreign enum field keeps the same proto3
presence as a local enum field. Wrapper messages belong to the package whose WIT source writes
the structural shape: a wrapper reached through a dependency's named `list`, `option`, or
`result` type lives in that dependency's `.proto` package (and is referenced cross-package by
the root and other dependents, with an `import` line), while a structural shape written in the
root lives in the root's `.proto` package even when its element type is foreign. A dependency's
`.proto` file and glue surface are therefore a function of the dependency alone, and a
dependency byte-list alias (including through an alias of `u8`) projects directly to `bytes` /
`Vec<u8>` in every direct and singular position.

Dependency packages are generated on a best-effort basis: supported types and functions are
included, and unsupported unexposed items are skipped. A local `use` makes the referenced type
and every named type it reaches required; importing or exporting a dependency interface from a
root world makes every item in that interface required. Required unsupported items are refused
by the subset checker before generation instead of being omitted. `deps/` is one level deep, as
`wit-parser` defines; a repository-level multi-package workspace and a dependency-generation
command remain separate later work.

Glue uses the same package-qualified references. It emits a shared `witforge_deps.rs` module
with conversion functions for each dependency type and interface function; the root glue file
calls those functions for local values whose type is foreign. The generated proto module layout
for a dependency is `<proto-root>::<namespace>::<package>::v<major>`; when a contract has
dependencies, the root types use `<proto-root>::<root-namespace>::<root-package>::v<major>` so
that prost's package-relative references resolve. The plain dependency types remain at
`<plain-root>::witforge_deps::<namespace>_<package>::<interface>::<Type>`.

JSON Schema also emits one `<namespace>.<package>.v<major>.schema.json` file per dependency.
A root or dependency schema refers to a foreign definition with a relative file reference,
`<namespace>.<package>.v<major>.schema.json#/definitions/<interface>/<type>`, so a consumer can
resolve dependency values by keeping the generated schema files together. The version adapters
in `convert.rs` copy a value whose type is the same pinned dependency type in both versions; a
changed dependency type is not a local conversion and must be handled by a new contract type.

## Cross-version invariant

Within each major version of a WIT root, every live version directory MUST resolve the identical
pinned dependency graph. Identity covers the package namespace, name, and version; interface,
type, and function inventories and their source order; shapes and signatures; stability metadata;
and docs. Across different majors, a new major MAY add, remove, change, or upgrade dependencies:

- A dependency whose definition and transitive dependency subgraph are identical across every live
  major that uses it stays at the single shared binding path (`witforge_deps::<ns>_<pkg>`,
  `<ns>.<pkg>.v<dep_major>.proto`, `<ns>.<pkg>.v<dep_major>.schema.json`).
- A dependency that differs across live majors (or transitively references one) is scoped per root
  major under `witforge_deps::v<root_major>::<ns>_<pkg>` in plain Rust and glue (with canonical
  `pub use super::v<owner>::<ns>_<pkg>;` re-exports whenever two live majors share an identical
  definition while a third differs). Its protobuf and JSON Schema files use owner-major
  disambiguation (`v<owner>.<ns>.<pkg>.v<dep_major>.proto`, `package <ns>.<pkg>.v<dep_major>_m<owner>;`,
  `v<owner>.<ns>.<pkg>.v<dep_major>.schema.json`) only when two live majors with different
  definitions share the same dependency major version.
- In `convert.rs`, a foreign type converts across a major boundary only when both majors pin the
  identical dependency package and transitive subgraph; otherwise types referencing it are omitted
  from cross-major adapters and listed with the other cross-major omissions.

One dependency graph MUST contain at most one version of a `namespace:name` package. A dependency
type's identity is `namespace:name/interface/type`; the version is not part of a lockfile key or
canonical type. Two versions under `deps/` would therefore collapse into one package in the
lockfile and every generated surface. This applies to every package under `deps/`, including one
nothing refers to, because every package contributes to the generated surface. The diagnostic
names the package, every version found, and the `deps/` entry that supplied each copy; `check`,
`lock`, `diff`, and every generator refuse it with exit `1` from the shared loader.

A dependency MUST NOT use the root package's `namespace:name`. The root and a dependency with one
identity would generate one module, one proto package, and one schema namespace. Rename one side
instead. Two packages with the same name in different namespaces (`example:shared` and
`acme:shared`) are different packages and are accepted.

Two copies of one package at the same version are a duplicate package as well, whether they sit
in two `deps/` entries or one entry has an accidental copy. `wit-parser` already refuses that
shape; witforge's pre-scan reports it as the same contract violation before resolution so the
diagnostic names both entries and the exit status is `1` rather than a parse error.

A new major of a dependency is adopted by every dependent at once. Updating one dependent only,
or keeping two versions in one graph so that both can be used, requires separate WIT roots and
generated binding surfaces.

`rust`, `proto`, `schema`, and `glue` compare every version with the first and refuse the first
changed package, version, interface, type, or function. `diff` applies the same comparison and
refuses mismatched graphs instead of reporting a local contract as compatible. A dependency pin
change therefore requires a separate WIT root and generated binding surface, or a single change
that updates the copied `deps/` graph in every version directory of the root at once; partial
adoption is refused. `check --against` applies the same comparison when its baseline is a
version directory.

`witforge check` accepts one version directory. It proves that directory loads, stays in the
supported subset, has a current lockfile, and—when `--against` supplies a version directory's
`wire.lock`—continues the prior lockfile and resolves the identical pinned dependency graph
using the generators' shared comparison. A baseline lockfile copied outside a version directory
has no WIT beside it; `check` reports that the graph cannot be compared, and CI MUST also run a
generator `--check` over the WIT root and SHOULD run `diff` for a reviewed version pair.

Every package loaded from the standard `deps/` directory contributes its supported generated
surface to this graph, including a package the root does not reference. Every package under
`deps/` (referenced or not) MUST therefore declare a `<major>.<minor>.0` version; a missing
version or a patch, pre-release, or build-metadata version is a subset diagnostic (exit `1`, code
`WF1012`, formerly `WF1001`) naming the package, its version, and its `deps/` entry. Root reachability instead
decides which unsupported items inside a dependency are required and therefore refused: a local
`use` reaches its type transitively, and a world import or export (including an implicit import
synthesized when a world exports or imports an interface or `use`s a type that depends on a
dependency interface) reaches the complete dependency interface and names the world item that
pulled it in.


## Lockfiles and release freezes

A local lockfile records only members defined by the local package. A local field, parameter,
method, or variant payload whose type is foreign records the full canonical type (with package
identity), so a retype from one dependency type to another is a wire change even when both names
end in the same suffix. Dependencies do not get a second lockfile here; their proto numbers are
derived deterministically from their own source order. Dependency identity has no version, so
one graph may contain only one version of a `namespace:name` package; a duplicate version or a
dependency on the root package's name is refused before the lockfile is read or written.

A dependency under a released version's `deps/` directory is part of that frozen version
directory: changing it after release fails the release-freeze check. Dependencies placed beside
the version directory (outside it) are not covered; keep them in the version directory. A
multi-package workspace, root-level shared deps, registries, and dependency cache/pinning tools
are explicitly later work, not part of this compatibility rule.

## Refusals and tests

- A nested `package` block in the root source remains refused; only packages under `deps/` are
  dependencies.
- A world may import or export a named interface from a standard `deps/` package. Interfaces
  from a nested package outside `deps/` remain refused.
- Dependency packages must use the supported subset for every item a root type reaches
  transitively. A world import or export reaches the dependency interface's complete API;
  unsupported items elsewhere in an unexposed dependency remain skippable.
- Every emitter refuses package or module names that normalize to the same generated name.
  These contract failures use exit `1`, including cross-version dependency disagreement.
- Two versions of one `namespace:name`, two copies of one package at one version, and a
  dependency that reuses the root package's `namespace:name` are contract failures with exit
  `1`. The first names the package, both versions, and both `deps/` entries; the root-name case
  names the package and says the root and a dependency cannot share an identity.
- Tests cover a local package importing a pinned dependency, a missing dependency, two
  dependencies declaring the same package name, two versions of one dependency, a dependency
  with the root's name at an earlier, equal, and later version, equal names in different
  namespaces, aliases, lists, options, results, variants,
  cross-dependency references, hyphenated package names, unused unsupported dependency items,
  rejected unsupported types reached through a local `use`, rejected unsupported items in a
  world-exposed dependency interface, dependency generated-name collisions, foreign world
  imports and exports, generated Rust/protobuf/glue compilation and round-trips, and version
  conversion with foreign values.
