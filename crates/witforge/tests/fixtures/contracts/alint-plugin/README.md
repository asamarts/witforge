# alint plugin contract fixture

This fixture is a representative contract derived from alint's accepted plugin-contract
decisions and the contract-side types in `alint-core` while the standalone
`alint:plugin@0.1.0` WIT draft is unavailable.

## Sources

- alint `docs/adr/0024-relation-ir-decisions-after-review.md`, especially decisions 1, 5, and 9
  (flat values, plugin contract path, resource and row shapes).
- alint `docs/design/plugin-architecture.md`, sections 3.2, 7, and 8.
- alint `docs/design/v0.21/README.md`, scope and plugin-contract sections.
- alint `crates/alint-core/src/plugin_api/contract.rs` and `value.rs` at
  `a13703a25a83735c84bc2ad577a397b02d4d08f6` (2026-10-27).

## Design findings

The source is Rust contract types, not a written WIT package, so this fixture records the
adaptations needed to express it in WIT instead of silently editing them away:

1. **`usize` has no WIT equivalent.** Rust `usize` values (`Resource.index`, line and column
   positions, limits) are represented as `u32` for positional values and `u64` for limits and
   byte offsets. The wire contract therefore fixes a width where the Rust API used the host
   width.
2. **The Rust `FlatValue` is recursive in memory.** WIT has no recursive types, so the fixture
   carries a flat `list<node>` and node references are indices. `node.list` and `node.record`
   use escaped WIT keywords (`%list`, `%record`) for their WIT names.
3. **Rust traits become roles.** The `Adapter` and `Analyzer` traits are represented as the
   `adapter-api` and `analyzer-api` interfaces, exported by the `adapter` and `analyzer` worlds.
   The `fixer` role is deferred by ADR-0024 and is intentionally absent from this 0.1 fixture.
4. **Field names that collide with WIT keywords are escaped.** The `Row.resource` and
   `AdapterInput.resource` fields are written `%resource`; the model exposes the raw names.
5. **No subset refusal was required.** The derived contract uses records, variants, options,
   lists, results, strings, booleans, byte lists, and fixed-width integers only. Resources,
   handles, `future`, `stream`, `error-context`, maps, fixed-length lists, tuples, `char`, and
   foreign or world-defined types are not needed. If a future alint draft adds one, record it
   here as a design finding and change the subset deliberately rather than silently dropping
   the construct.
6. **The checked-in lockfile is transitional.** It is the current v1 lockfile produced by the
   existing CLI so `witforge check` can exercise the fixture end to end. Lockfile v2 and its
   canonical types/functions/parameters replace it during the lockfile-v2 slice.
