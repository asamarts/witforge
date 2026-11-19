---
layout: default
title: Contract evolution
---

# Contract evolution

## Add a minor version

A new minor is a new directory that starts as a copy of the previous version, lockfile included:

```sh
mkdir wit/v0.2
cp wit/v0.1/contract.wit wit/v0.1/wire.lock wit/v0.2/
# Edit wit/v0.2/contract.wit: set the package version to 0.2.0, then change the contract.
witforge lock wit/v0.2 --against wit/v0.1/wire.lock
witforge check wit/v0.2 --against wit/v0.1/wire.lock
witforge diff wit/v0.1 wit/v0.2
witforge diff wit/v0.1 wit/v0.2 --check   # exits 1 if the change is breaking
```

The package version in `contract.wit` must match the directory name; with the copied `@0.1.0`
still in place, every command refuses `wit/v0.2`. Copy every `.wit` file and the `deps/`
directory when the version has more than one file or uses dependencies.

Never create a minor version with a fresh lockfile (`lock --first`). Copying preserves protobuf field numbers, method IDs, removed-member reservations, and declaration-order history.

`diff --check` exits 1 on a wire-, source-, or generated-API-breaking change. Adding a record
field is source-breaking even when its type is defaultable, because Rust code that constructs
the record must be updated.

## Version adapters

`witforge rust` emits `convert.rs` for adjacent versions in both directions.

Old-to-new conversion defaults an added record field only when its type is `option<T>` (to `None`) or `list<T>` (to an empty list), and sets a flag added to an existing flags type to `false`. A required addition of any other type, including a field whose type is a flags, record, or scalar type, is refused when the adapters are generated.

New-to-old conversion drops fields and flags that the older version does not have; it does not report them. It returns `MissingMember` when the older version requires a field the newer version removed. In either direction, a value whose enum or variant case the target version does not have returns `UnknownCase`. Retypes and kind changes are refused.

## Released versions

List immutable version directories in `wit/released`. Do not edit their WIT, lockfile, dependency packages, or generated per-version Rust module. Create a new version directory instead.

Use the repository release-freeze script or an equivalent history check in CI.
