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
- [x] Evaluate a leaner glue surface that skips trivial structural aliases (the glue Option C)
      while keeping message-backed and `use` aliases. Declined for 1.0: no consumer requested the
      reduced surface, and the complete conversion surface is already tested and stable.
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
- [x] JSON Schema emitter (`W6-008`, `W10-108`): Draft 7 default and opt-in Draft 2020-12 schemas,
      one file per version, with all named definitions and member descriptions.
- [ ] Replacing the lockfile with in-source numbers if the WIT annotations proposal
      (component-model issue 695) lands with a suitable annotation.
- [ ] Wider WIT coverage (maps, resources as opaque handles) only on demand.
- [x] 1.0 criteria published: stable library API, conformance matrix, release gates, and
      deprecation runways; 1.0.0 has not yet been released.

## 0.2.0: focused compiler release (done)

- [x] Release the current compiler surface: conversions, order fingerprints, JSON Schema, world
      metadata, `witforge.toml`, and the CI plan gate.
- [x] Add a quickstart, examples, and a clear scope statement before the release.

## 0.3.0: change-review and CI tooling (done)

- [x] `witforge diff` with human and JSON breaking-change reports.
- [x] Structured diagnostics and SARIF output.
- [x] GitHub Action and prebuilt binaries.
- [x] Fuzzing, benchmarks, and dependency/security gates.
- [x] Release 0.3.0.

## 0.4.0: WIT dependencies, foreign interfaces, and Rust Serde (done)

- [x] Standard `wit/deps` package dependencies with explicit package identity (`W9-001`).
- [x] Generated dependency Rust and protobuf bindings (`W9-004`).
- [x] Foreign interfaces and dependency glue (`W9-005`).
- [x] Opt-in per-project Serde generation (`W9-002`).
- [x] Release 0.4.0 (`W9-003`).

## 0.5.0: diff review (released)

This is a summary of what tag `v0.5.0` contains; the changelog's 0.5.0 section is the full list.

- [x] `diff` reports guaranteed down-conversion failures as notes and refuses a stale lockfile
      on a direct version-directory endpoint.
- [x] The Component Model guide is corrected; the semantic decode rules for non-Rust protobuf
      implementations and the three JSON projections are documented; generated `convert.rs`
      no longer ends with an extra blank line.
- [x] Release 0.5.0 (2026-11-17), with alint moved to the release and its generated output
      regenerated.

## 0.6.0: assurance and audit remediation (released)

This is a summary of what tag `v0.6.0` contains; the changelog's 0.6.0 section is the full list.

- [x] `diff` reports required-addition generation blockers, rejects reversed endpoints, and
      gives actionable missing-lock guidance.
- [x] Generated writes are atomic; released version directories reject symlinks inside them;
      the Action bounds its network requests; the fuzz workspace is integrated with CI,
      Dependabot, and the security gate; dependency-graph, conversion, output, and diagnostic
      coverage is expanded.
- [x] Generated-output excerpts in the README and the output guide, the documented command
      sequences, and current-version references are checked in CI; shared emitter helpers
      remove duplicated naming and type rendering without changing generated output.
- [x] Diff and version-conversion benchmark budgets cover a generated two-version fixture.
- [x] The [second audit](docs/design/audit-2026-11-18-second.md)'s release blockers (wire-identity
      and lockfile hardening, unified prost/Rust identifier conversion, generator-refusal parity in
      `check`, clean `rustfmt`/Clippy/`missing_docs` Rust output, hardened `convert.rs`, complete
      JSON Schema `result` shapes and `f32` bounds, contract-scoped ownership headers, canonical
      output-root guards, deterministic multi-file loading, and the `prost` compile matrix) and
      release 0.6.0 (2026-11-21, `W10-069`).

## 0.7.0: final pre-1.0 surface (released)

This is a summary of what tag `v0.7.0` contains; the changelog's 0.7.0 section is the full list.

- [x] Make public model, lockfile, and diagnostic data extensible; move check/lock workflows and
      typed findings into the library; expose deterministic in-memory package loading.
- [x] Carry `@since`, `@unstable`, and `@deprecated` through validation, Rust, protobuf, JSON
      Schema, world metadata, and non-breaking stability diffs.
- [x] Complete the second-audit command workflows, version retirement, cross-major dependency and
      conversion handling, diagnostics, Action hardening, supply-chain provenance, documentation,
      fuzzing, and release gates.
- [x] Release 0.7.0 (2026-11-23, `W10-099`) as the final planned pre-1.0 Rust-library break.

## 0.8.0: lifecycle CLI, contract IR, `no_std`, `witforge-build`, Draft 2020-12, and static `musl`

Strictly additive pre-1.0 minor release over 0.7.0's public API; the changelog's 0.8.0 section
has the full list, and release tracking lives in `W10-123`:

- [x] Lifecycle CLI (`witforge init`, `witforge retire`, `witforge new-version --from`/`--released`),
      versioned JSON contract IR (`witforge inspect` / `witforge::inspect`), opt-in `no_std` +
      `alloc` Rust/convert/glue output, optional `witforge-build` helper crate, JSON Schema
      Draft 2020-12 and member descriptions, static `musl` Linux archives alongside `gnu`, and
      Action `check: true` / `generators: all` / default `github-token` (`W10-102`–`W10-122`,
      `W10-124`).

## 1.0 and beyond

- [ ] External consumer validation and the 1.0.0 release are deferred while proactive outreach is
      paused; the 1.0 release criteria (including the independent-adoption and real-evolution
      gates) live in [release-1.0.md](docs/design/release-1.0.md) (`W10-001`, `W10-002`,
      `W10-098`, `W10-100`).
- [ ] Record one real consumer evolution across two witforge releases (`W10-100`) after 0.7.0.
- [ ] Continue bounded implementation work that does not speculate about consumer demand.
- [ ] Keep plugin ABI, LSP, registry/playground, adopter-guide, cross-language profile, and aligned
      JSON work behind their existing evidence or 1.0 triggers.
