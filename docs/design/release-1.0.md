# 1.0 release criteria and deprecation runways

Status: criteria published 2026-11-01; **1.0.0 has not been released**.

1.0 is a promise about the library API, generated output, and the conformance suite. Feature
completeness alone is not enough.

## Release criteria

- [ ] the public API inventory in [`public-api.md`](public-api.md) is reviewed and documented;
      `cargo doc` and `missing_docs` are clean
- [ ] the boundary error policy is published: `anyhow::Error` plus the stable `ContractError`
      classification, with no unclassified contract failure
- [ ] the conformance matrix and `tests/conformance.rs` cover every supported WIT construct and
      every emitter mapping row
- [ ] the release test matrix is green on Linux, macOS, and Windows, and on MSRV 1.88
- [ ] `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
      `cargo test --workspace --locked`, `RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps`,
      `ci/check-generated.sh`, and `ci/check-release-freeze.sh` pass
- [ ] the alint consumer pin and its CI are green against the 1.0 candidate
- [ ] `CHANGELOG.md`, README status, workspace versions, and the release checklist agree
- [ ] the protobuf compatibility check is clean for every released major
- [ ] aplan `check`, `verify`, `export-agents-md --check`, and `check-docs` pass
- [ ] the release owner records the tag commit, CI run, and publish run in the changelog section

## Deprecation runways

### Library

- A public item is deprecated with `#[deprecated]` and a replacement, then remains available for
  at least one minor release before removal in the next major.
- Removing or changing a public item, adding a field to a constructible public struct, or adding
  a variant to an exhaustively matched public enum is a major-version change.
- MSRV increases are a minor release and are documented in the changelog and release notes.

### Generated output and wire

- A released contract version directory, its lockfile, and its per-version Rust module are
  immutable; changes require a new version directory.
- Wire numbers, reserved names, and proto method ids are never removed or reused. A removed
  member stays reserved; removing a type or function keeps its method number reserved.
- Shared generated output (`mod.rs`, `convert.rs`, `worlds.rs`, per-major proto, glue) may gain
  additions when a new version is added, but released members remain protected by the lockfile
  and `--check`.
- CLI flags are deprecated one release before removal, except security fixes; a removed flag
  remains documented in the changelog.

### Contract versions

- A new major starts a fresh lockfile and a new protobuf package; a previous-major adapter is
  documented for a stated period.
- A new minor copies the previous lockfile and uses `lock --against`; declaration-order
  fingerprints and the append-only rules protect common members.
