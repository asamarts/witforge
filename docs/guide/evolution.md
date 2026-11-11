---
layout: default
title: Contract evolution
---

# Contract evolution

## Add a minor version

```sh
cp wit/v0.1/wire.lock wit/v0.2/wire.lock
witforge lock wit/v0.2 --against wit/v0.1/wire.lock
witforge check wit/v0.2 --against wit/v0.1/wire.lock
witforge diff wit/v0.1 wit/v0.2
witforge diff wit/v0.1 wit/v0.2 --check
```

Never create a minor version with a fresh lockfile. Copying preserves protobuf field numbers, method IDs, removed-member reservations, and declaration-order history.

## Version adapters

`witforge rust` emits `convert.rs` for adjacent versions in both directions.

Old-to-new conversion can default added `option<T>`, `list<T>`, and flags. New-to-old conversion reports data that cannot be represented. Added/removed enum or variant cases report `UnknownCase`. Retypes and kind changes are refused.

## Released versions

List immutable version directories in `wit/released`. Do not edit their WIT, lockfile, dependency packages, or generated per-version Rust module. Create a new version directory instead.

Use the repository release-freeze script or an equivalent history check in CI.
