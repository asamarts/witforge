---
layout: default
title: Contract evolution
---

# Contract evolution

## Add a minor version

A new minor is a new directory that starts as a copy of the previous version, lockfile included
(`witforge new-version wit` automates this from the latest version, and
`witforge new-version wit --from v0.2` creates a maintenance minor `v0.3` on an earlier major line
even after `v1.0` exists):

```sh
mkdir wit/v0.2
cp wit/v0.1/contract.wit wit/v0.1/wire.lock wit/v0.2/
# Edit wit/v0.2/contract.wit: set the package version to 0.2.0, then change the contract.
witforge lock wit/v0.2 --against wit/v0.1/wire.lock
witforge check wit/v0.2 --against wit/v0.1/wire.lock
witforge diff wit/v0.1 wit/v0.2
witforge diff wit/v0.1 wit/v0.2 --check   # exits 1 if the change is breaking
```

To compare your current working tree directly against a Git base revision without checking out a
second tree (for example in a pull request), run
`witforge diff --git-base origin/main wit --check=wire,decode`.

The package version in `contract.wit` must match the directory name; with the copied `@0.1.0`
still in place, every command refuses `wit/v0.2`. Copy every `.wit` file and the `deps/`
directory when the version has more than one file or uses dependencies.

Never create a minor version with a fresh lockfile (`lock --first`). Copying preserves protobuf field numbers, method IDs, removed-member reservations, and declaration-order history.

`diff --check` exits 1 on a wire-, source-, or generated-API-breaking change. Adding a record
field is source-breaking even when its type is defaultable, because Rust code that constructs
the record must be updated.

## Wire identity vs. decode compatibility

`witforge diff` separates **wire identity** (`wire`) from **protobuf binary decode compatibility**
(`decode`):

- **Wire identity (`wire`)** checks whether lockfile numbers, kinds, and canonical types changed
  meaning. Adding a new field or enum case gets a fresh wire number, so it is `wire:compatible`.
- **Decode compatibility (`decode`)** checks whether two separate processes running different
  minors of the same major can actually decode each other's protobuf binary messages under the
  decode profile ([`non-rust.md`](non-rust.md) / generated Rust glue):
  - `new<-old` (`new_reads_old`): whether a newer reader accepts messages sent by an older writer.
    Adding a required record field or parameter — including required `list<u8>` (`optional bytes`),
    `list<T, N>`, or `char` — breaks `new<-old` (`MissingField` or `InvalidLength`), as does removing an enum case, variant case, or function.
    Adding an `option<T>` field, a non-byte `list<T>` field, a `map<K, V>` field, or a flag stays compatible.
  - `old<-new` (`old_reads_new`): whether an older reader accepts messages sent by a newer writer.
    Adding an enum case, variant case, or function breaks `old<-new` (`UnknownEnumValue` /
    `UnknownCase`), as does removing a required field (`MissingField`). Adding any field or flag
    stays compatible in protobuf **binary** encoding because unknown fields are ignored (note that
    strict proto3-JSON parsers such as `pbjson` reject unknown JSON field names by default).

`decode` applies within one major (two majors have two distinct protobuf packages negotiated
separately) and is distinct from in-process `convert.rs` adapters. Bare `witforge diff --check`
gates on `wire,source,api` and does not gate on `decode` by default; pass `--check=decode` or
`--check=wire,decode` when rolling upgrades require two services at different minors to talk on
the wire.

## Version adapters

`witforge rust` emits `convert.rs` for adjacent versions in both directions.

Old-to-new conversion defaults an added record field only when its type is `option<T>` (to `None`), `list<T>` (to an empty list), or `map<K, V>` (to an empty `BTreeMap`), and sets a flag added to an existing flags type to `false`. A required addition of any other type, including a field whose type is a fixed-length list, flags, record, or scalar type, is refused when the adapters are generated.

New-to-old conversion drops fields and flags that the older version does not have; it does not report them. It returns `MissingMember` when the older version requires a field the newer version removed. In either direction, a value whose enum or variant case the target version does not have returns `UnknownCase`. Retypes and kind changes are refused.

## Starting a new major

A breaking change — retyping a field, changing a type's kind, adding a required field without a
default, renaming a type, or upgrading a dependency — belongs in a new major version (`v1.0` after
`v0.x`, or `v2.0` after `v1.x`). A new major starts with a fresh lockfile (`--first`) and gets its
own protobuf package (`v1.proto`) and glue module (`v1.rs`).

### In the same WIT root (recommended)

Keeping the new major in the same WIT root gives you all versions under one generated module tree
and generates adapters automatically for every type that still converts:

```sh
witforge new-version wit --major
# Edit wit/v1.0/contract.wit to make breaking changes (and wit/v1.0/deps/ if upgrading dependencies).
witforge lock wit/v1.0 --first
witforge check wit
witforge diff wit/v0.2 wit/v1.0
witforge rust wit --out generated/rust
```

Across a major boundary:

- **Plain types always generate.** `witforge rust` never refuses a major boundary, so `v1_0.rs`
  and every earlier live version's types are always written.
- **Partial adapters generate per direction.** Types that still convert under the normal rules get
  adapters in `convert.rs`. Types that do not — retypes, kind changes, required additions (in the
  old-to-new direction only; new-to-old drops the new field and still generates), payload-presence
  changes, types with no namesake, types using an upgraded dependency, and any containing type
  transitively — are omitted in that direction.
- **Every omission is listed twice.** The top of `convert.rs` carries a sorted, deterministic
  comment block listing every omitted `(from -> to, type, reason)` pair (`--check` covers it), and
  `witforge diff wit/v0.2 wit/v1.0` reports the exact same list. Write manual adapters for those
  types in your own crate beside the generated ones.
- **Dependencies may change or upgrade.** Within one major, every minor still shares an identical
  pinned dependency graph. Across majors, unchanged dependencies stay in `witforge_deps::<dep>`
  and still convert across the major; dependencies that changed or were upgraded move into
  per-major modules (`witforge_deps::v0::<dep>`, `witforge_deps::v1::<dep>`), and proto/schema
  files disambiguate only if two live majors pin conflicting definitions with the same dependency
  major version.
- **`diff --check` still reports breaking changes.** Because a new major is expected to break, run
  `witforge diff wit/v0.2 wit/v1.0` without `--check` when reviewing a major boundary.

### Retiring old versions

Once consumers no longer need the oldest versions in a WIT root, retire them with
`witforge retire wit/v0.1` (which validates release status, contiguous oldest-prefix ordering, and
at least one remaining live version before atomically marking `<entry> retired` in `wit/released`,
first supported in 0.7.0; earlier releases reject the syntax):

```text
v0.1 retired
v0.2 retired
v1.0
```

- Only a contiguous oldest prefix of versions may be retired, and at least one live version must
  remain. Retiring `v0.2` while `v0.1` is still live (or retiring every version) is refused.
- Never delete an entry or directory from `wit/released`. Retired directories remain in Git and
  stay frozen; `witforge freeze` accepts the one-way transition from live to `retired` and refuses
  un-retiring (`retired` back to live) or editing a retired directory.
- Retired versions stop being generated: running the generators removes their plain Rust modules,
  JSON Schema files, and adapters through normal stale-file cleanup. Retiring an older minor of a
  live major leaves `v<major>.proto` unchanged (the highest minor carries every reservation);
  retiring every version of a major removes that major's `.proto` file and glue module as well.

### Separate WIT root per major (alternative)

If you prefer completely isolated generated crates or output trees with no shared `convert.rs` or
`witforge_deps.rs`, place each major in its own WIT root (`wit-v1/`, `wit-v2/`) with separate
output directories (or separate `[project.<name>]` entries in `witforge.toml`) and write any
cross-major bridge in your own crate.

## Released versions

List immutable version directories in `wit/released` using `witforge release wit/v0.1` (which
validates the directory and predecessor before appending to `wit/released`). Do not edit a
released version's WIT, lockfile, dependency packages, or generated per-version Rust module.
Create a new version directory instead (`witforge new-version wit` for a minor, or
`witforge new-version wit --major` for a new major).

Use `witforge freeze` (with `fetch-depth: 0`) in CI to verify released history (including one-way
live-to-retired transitions).
