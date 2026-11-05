# witforge roadmap

witforge is built for its first consumer, the alint plugin contract, and its scope stays limited
to what that contract needs until the contract's first version ships. Estimates are
engineer-weeks for one experienced Rust engineer.

Task-level planning, dependencies, and closure evidence live in [`aplan/`](aplan/README.md) and
are validated with `aplan check` and `aplan verify`. This document is the narrative sequence.

## 0.0.x: foundations (done)

- [x] Load a WIT package with every feature enabled (`@unstable` items kept).
- [x] Restricted-subset checker with a refuse-unknown catch-all.
- [x] Contract versions restricted to `<major>.<minor>.0`; patch, pre-release, and build metadata
      shapes are refused.
- [x] Append-only wire lockfile: parse, render, reconcile, drift check, append-only check
      against an older lockfile.
- [x] `witforge check` and `witforge lock`.
- [x] CI: fmt, pedantic Clippy, rustdoc, tests, and the MSRV 1.88 build and tests (the
      `wit-parser` floor); on a self-hosted Linux runner while the repository was private, on
      GitHub-hosted runners since it went public ([ci/README.md](ci/README.md)).
- [x] Rust edition 2024.
- [x] Specification for the internal model, lockfile v2, and the Rust/protobuf/glue emitter
      conventions ([model.md](docs/design/model.md), [lockfile.md](docs/design/lockfile.md),
      [emitters.md](docs/design/emitters.md)).
- [x] Implement lockfile v2 as specified: typed keys and entries, member kinds, canonical types,
      function method ids and parameters, strict parsing/rendering, number limits, `--first`,
      `--against`, `wit/released`, and the CLI exit/write rules, with property and CLI tests.

## 0.1: the MVP alint needs (about 13 to 20 weeks in total)

- [x] Internal model as specified in [model.md](docs/design/model.md): resolved types, docs,
      stability, aliases, worlds, canonical type representation, lockfile member extraction, and
      diagnostics, independent of `wit-parser`'s types.
- [x] Rust emitter as specified in [emitters.md](docs/design/emitters.md): per-interface
      submodules in one module per version, plain exhaustive types, traits with `&self` and owned
      values, docs carried from `///` comments, deterministic `mod.rs`/version files.
- [x] Protobuf emitter as specified in [emitters.md](docs/design/emitters.md): one file per major,
      interface-prefixed messages, messages and enums numbered from `wire.lock`, `reserved` for
      removed members, wrappers for nested `list` and `option`, `oneof` for variants and
      `result`, closed enums with a zero `UNSPECIFIED` value, request/response messages, and a
      method-id enum per interface.
- [x] Protobuf glue as specified in [emitters.md](docs/design/emitters.md): free-function type
      conversions, per-function request/response helpers and method constants, the shared
      `GlueError`, and `--plain-root`/`--proto-root` output, exercised by round-trip tests through
      `prost`/`pbjson`.
- [x] `--check` mode for generated outputs (stale output fails CI); LF-only, deterministic output.
- [x] Golden tests for every mapping row and round-trip tests through the protobuf glue (Rust
      value to protobuf to Rust value; proto3-JSON and back).
- [x] CI guard that released version directories (WIT and lockfile) never change after the
      commit that released them, read from the history of `wit/released`, which must stay
      append-only; generators that refuse a minor whose lockfile does not continue the previous
      minor's, so wire numbers in the shared per-major proto/glue output never change; and
      documentation for running `buf breaking` on the emitted schema as an optional extra check.
      Per-version Rust modules are reproduced by `--check`, and a consumer's CI freezes its
      committed copies.
- [x] 0.1 generation/ergonomics baseline and deferred-option triage (`W5-004`), with every
      pre-golive option assigned to its owning task and trigger.
- [x] Public repository and the first crates.io release once alint's contract `0.x` consumes it.
      The repository is public, CI runs on GitHub-hosted Linux, macOS, and Windows runners, the
      self-hosted runner is torn down, and 0.1.0 is on crates.io and tagged `v0.1.0`
      (2026-10-31).

## Pre-golive (before 1.0)

- [x] Declaration-order fingerprinting (lockfile order Option C): an additive per-parent order
      section makes `check --against` catch reorders of common members while ignoring additions
      and removals.
- [x] Measure owned Rust parameters and consider borrowed parameters (`&str`, `&[T]`):
      owned remains the 0.x/1.0 policy; borrowed needs a measured consumer path and a new
      contract version.
- [ ] Evaluate a leaner glue surface that skips trivial structural aliases (the glue Option C)
      while keeping message-backed and `use` aliases.
- [x] Conditional `Eq`/`Hash` and a narrow `Default` policy: declined for 0.x/1.0; requires a
      consumer request and a new contract version with an explicit float/default policy.
- [x] Generated world metadata (`W6-004`): additive shared `worlds.rs` with explicit per-version
      imports and exports.
- [x] `witforge.toml` configuration for multiple roots and emitters (`W6-005`): explicit projects,
      relative paths, and CLI flags override config values.
- [x] Docs-only patch versions and a type/interface/world inventory for memberless definitions:
      declined for 0.x/1.0; drafts accept docs-only changes, memberless additions have no wire
      entry, and released docs are freeze-guarded.

## After 0.1

- [x] 0.1.1: the post-publish review fixes (`W6-010`, `W6-011`), the first release published by
      `release.yml` through crates.io trusted publishing (`W6-012`).
- [x] 0.1.2: the fourth-review validation and refusal fixes (`W6-013`), released through the
      same trusted-publishing workflow (`W6-014`).
- [x] Generated conversions between contract versions (`W6-007`): `rust` emits `convert.rs` with
      old-to-new and new-to-old adapters for every adjacent version pair.
- [x] JSON Schema emitter (`W6-008`): Draft 7 schemas, one file per version, with all named definitions.
- [ ] Replacing the lockfile with in-source numbers if the WIT annotations proposal
      (component-model issue 695) lands with a suitable annotation.
- [ ] Wider WIT coverage (maps, resources as opaque handles) only on demand.
- [x] 1.0 criteria published: stable library API, conformance matrix, release gates, and
      deprecation runways; 1.0.0 has not yet been released.

## 0.2.0: focused compiler release (done)

- [x] Release the current compiler surface: conversions, order fingerprints, JSON Schema, world
      metadata, `witforge.toml`, and the CI plan gate.
- [x] Add a quickstart, examples, and a clear scope statement before the release.

## 0.3.0: change-review and CI tooling

- [x] `witforge diff` with human and JSON breaking-change reports.
- [ ] Structured diagnostics and SARIF output.
- [ ] GitHub Action, prebuilt binaries, fuzzing, benchmarks, and dependency/security gates.
- [ ] Release 0.3.0.

## 0.4.0: WIT dependencies and Rust Serde

- [ ] Standard `wit/deps` package dependencies with explicit package identity.
- [ ] Opt-in per-project Serde generation.
- [ ] Release 0.4.0.

## 1.0 and beyond

- [ ] Validate 1.0 with additional external consumers, then release 1.0.0.
- [ ] Evaluate plugin ABI, LSP, and registry/playground after 1.0; do not build them
      speculatively.
