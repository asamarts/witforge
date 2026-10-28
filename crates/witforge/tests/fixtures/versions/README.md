# Evolution fixture

This fixture exercises the lockfile v2 lifecycle:

- `v0.1` contains `name`, `old-field`, and `rename-me`.
- The `v0.1` lock also carries a released reserved `removed` entry, as if an
  earlier version had removed it.
- `v0.2` adds `new-field`, removes `old-field`, renames `rename-me` to `renamed`,
  and keeps the `removed` name reserved.
- `released` lists `v0.1`, so `lock` must refuse that directory.
- `reintroduced/contract.wit` is a v0.2 source overlay that re-adds `removed`;
  against the v0.1 baseline it must be refused as a released reintroduction.
- `retyped/contract.wit` is a v0.2 source overlay that changes the draft
  `new-field` type; `lock --against` must update that draft.

The checked-in v0.2 lock is generated with:

```text
witforge lock v0.2 --against v0.1/wire.lock
```
