# WIT package dependencies

Status: implemented 2026-11-06 (`W9-001`).

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

Local names keep the existing `interface/type` form. The package version is not part of a
lockfile key or canonical type: the dependency pin is the versioned `use` path plus the content
under `deps/`, and a dependency version bump must not renumber a local contract.

## Generated references

The emitters deliberately keep dependency references external rather than copying or
redefining another package's types:

- Rust: `super::super::witforge_deps::<namespace>_<package>::<interface>::<Type>`.
- Protobuf: `<namespace>.<package>.v<major>.<Interface><Type>`.

The Rust emitter writes the dependency modules into `witforge_deps.rs`; the protobuf emitter
writes one `<namespace>.<package>.v<major>.proto` file per dependency and imports it from the
root schema. `deps/` is one level deep, as `wit-parser` defines. JSON Schema uses an external
`urn:witforge:<qualified-name>` reference. Glue for foreign values requires the dependency's
glue and is outside this task's generated Rust/protobuf surface. A repository-level multi-package
workspace and a dependency-generation command remain separate later work.

## Lockfiles and release freezes

A local lockfile still records only members defined by the local package. A local field,
parameter, method, or variant payload whose type is foreign records the full canonical type
(with package identity), so a retype from one dependency type to another is a wire change even
when both names end in the same suffix. Dependencies do not get a second lockfile here.

A dependency under a released version's `deps/` directory is part of that frozen version
directory: changing it after release fails the release-freeze check. Dependencies placed beside
the version directory (outside it) are not covered; keep them in the version directory. A
multi-package workspace, root-level shared deps, registries, and dependency cache/pinning tools
are explicitly later work, not part of this compatibility rule.

## Refusals and tests

- A nested `package` block in the root source remains refused; only packages under `deps/` are
  dependencies.
- Foreign interfaces in worlds are still outside the subset; named dependency types are the
  supported surface.
- Tests cover a local package importing a pinned dependency, a missing dependency, and two
  dependencies declaring the same package name.
