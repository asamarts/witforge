# WIT package dependencies

Status: implemented 2026-11-06 (`W9-001`, `W9-004`, `W9-005`).

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
`deps/`, and a dependency version bump does not renumber local members. The cross-version rule
below still refuses that bump inside one generated WIT root.

## Generated references

The emitters keep dependency references external rather than copying or redefining another
package's types:

- Rust: `super::super::witforge_deps::<namespace>_<package>::<interface>::<Type>`.
  Inside a dependency's own generated module, a reference to another dependency is
  `super::super::<namespace>_<package>::<interface>::<Type>`.
- Protobuf: `<namespace>.<package>.v<major>.<Interface><Type>`, with `-` replaced by `_` in
  package and file names.

The Rust emitter writes dependency modules into `witforge_deps.rs`; the protobuf emitter writes
one `<namespace>.<package>.v<major>.proto` file per dependency and imports the schemas a file
actually references. World metadata uses the package-qualified interface name. A dependency
may use a type from another package under the same `deps/` directory; that import is emitted
from the dependency's schema as well. Alias, list, option, and result shapes resolve through
their dependency definitions, so a foreign alias maps to its underlying message or scalar
rather than to a non-existent alias message. A foreign enum field keeps the same proto3
presence as a local enum field.

Dependency packages are generated on a best-effort basis: supported types and functions are
included, and unsupported unused items are skipped. A local `use` makes the referenced type
required; importing or exporting a dependency interface from a root world makes every item in
that interface required. Required unsupported items are refused by the subset checker before
generation instead of being omitted. `deps/` is one level deep, as `wit-parser` defines; a
repository-level multi-package workspace and a dependency-generation command remain separate
later work.

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

Every version directory in one generated WIT root MUST resolve the identical pinned dependency
graph. Identity covers the package namespace, name, and version; interface, type, and function
inventories and source order; shapes and signatures; stability metadata; and docs. The emitters
write one shared dependency binding surface, so allowing any of those inputs to vary would make
the generated result depend on which contract version happened to supply it.

`rust`, `proto`, `schema`, and `glue` compare every version with the first and refuse the first
changed package, version, interface, type, or function. `diff` applies the same comparison and
refuses mismatched graphs instead of reporting a local contract as compatible. A dependency pin
change therefore requires a separate WIT root and generated binding surface.

`witforge check` accepts one version directory. It proves that directory loads, stays in the
supported subset, has a current lockfile, and—when `--against` is supplied—continues the prior
lockfile. It cannot prove cross-version dependency identity. CI MUST also run a generator
`--check` over the WIT root, and SHOULD run `diff` for a reviewed version pair.

Only dependencies reached by the root are included in this graph. Unsupported items elsewhere
in an otherwise unused package remain irrelevant; a world import or export reaches the complete
dependency interface.


## Lockfiles and release freezes

A local lockfile records only members defined by the local package. A local field, parameter,
method, or variant payload whose type is foreign records the full canonical type (with package
identity), so a retype from one dependency type to another is a wire change even when both names
end in the same suffix. Dependencies do not get a second lockfile here; their proto numbers are
derived deterministically from their own source order.

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
- Dependency packages must use the supported subset for any item the root actually references.
  A world import or export references the dependency interface's complete API; unsupported
  items elsewhere in an otherwise unused dependency remain skippable.
- Every emitter refuses package or module names that normalize to the same generated name.
  These contract failures use exit `1`, including cross-version dependency disagreement.
- Tests cover a local package importing a pinned dependency, a missing dependency, two
  dependencies declaring the same package name, aliases, lists, options, results, variants,
  cross-dependency references, hyphenated package names, unused unsupported dependency items,
  rejected unsupported items in a world-exposed dependency interface, dependency generated-name
  collisions, foreign world imports and exports, generated Rust/protobuf/glue compilation and
  round-trips, and version conversion with foreign values.
