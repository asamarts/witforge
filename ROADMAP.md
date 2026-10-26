# witforge roadmap

witforge is built for its first consumer, the alint plugin contract, and its scope stays limited
to what that contract needs until the contract's first version ships. Estimates are
engineer-weeks for one experienced Rust engineer.

## 0.0.x: foundations (in progress)

- [x] Load a WIT package with every feature enabled (`@unstable` items kept).
- [x] Restricted-subset checker with a refuse-unknown catch-all.
- [x] Contract versions restricted to `<major>.<minor>.0`; patch, pre-release, and build metadata
      shapes are refused.
- [x] Append-only wire lockfile: parse, render, reconcile, drift check, append-only check
      against an older lockfile.
- [x] `witforge check` and `witforge lock`.
- [x] CI: fmt, pedantic Clippy, rustdoc, tests, and the MSRV 1.88 build (the `wit-parser` floor),
      on a self-hosted Linux runner while the repository is private ([ci/README.md](ci/README.md)).
- [x] Rust edition 2024.
- [x] Specification for the internal model, lockfile v2, and the Rust/protobuf emitter
      conventions ([model.md](docs/design/model.md), [lockfile.md](docs/design/lockfile.md),
      [emitters.md](docs/design/emitters.md)).
- [ ] Implement lockfile v2 as specified: typed keys and entries, member kinds, canonical types,
      function method ids and parameters, strict parsing/rendering, number limits, `--first`,
      `--against`, `wit/released`, and the CLI exit/write rules, with property and CLI tests.

## 0.1: the MVP alint needs (about 13 to 20 weeks in total)

- [ ] Internal model as specified in [model.md](docs/design/model.md): resolved types, docs,
      stability, aliases, worlds, canonical type representation, lockfile member extraction, and
      diagnostics, independent of `wit-parser`'s types.
- [ ] Rust emitter as specified in [emitters.md](docs/design/emitters.md): per-interface
      submodules in one module per version, plain exhaustive types, traits with `&self` and owned
      values, docs carried from `///` comments, deterministic `mod.rs`/version files.
- [ ] Protobuf emitter as specified in [emitters.md](docs/design/emitters.md): one file per major,
      interface-prefixed messages, messages and enums numbered from `wire.lock`, `reserved` for
      removed members, wrappers for nested `list` and `option`, `oneof` for variants and
      `result`, closed enums with a zero `UNSPECIFIED` value, request/response messages, and a
      method-id enum per interface.
- [ ] Protobuf glue: decide and document the generated API (free functions vs conversions, error
      type, module paths) and implement conversions between the plain types and `prost`-generated
      types (built with `protox` and `pbjson-build`), carrying every decode rule in
      [emitters.md](docs/design/emitters.md).
- [ ] `--check` mode for generated outputs (stale output fails CI); LF-only, deterministic output.
- [ ] Golden tests for every mapping row and round-trip tests through the protobuf glue (Rust
      value to protobuf to Rust value; proto3-JSON and back).
- [ ] CI guard that released directories (WIT, lockfile, and per-version Rust module) never
      change against the merge base, that shared per-major proto files remain append-only, that
      `wit/released` is append-only, and documentation for running `buf breaking` on the emitted
      schema as an optional extra check.
- [ ] Public repository and the first crates.io release once alint's contract `0.x` consumes it;
      CI moves back to GitHub-hosted Linux, macOS, and Windows runners, and the self-hosted
      runner is torn down.

## Pre-golive (before 1.0)

- [ ] Evaluate declaration-order fingerprinting (lockfile order Option C), so a reorder in an
      unreleased version is caught by `check --against` rather than review alone.
- [ ] Measure the cost of owned Rust parameters and consider borrowed parameters (`&str`,
      `&[T]`) in a new contract version.
- [ ] Evaluate conditional `Eq`/`Hash` and a narrow `Default` policy for generated Rust types.
- [ ] Explore generated world metadata (the world Option D) if alint needs role information in
      generated code.
- [ ] Consider a `witforge.toml` configuration file for multiple roots and emitters.
- [ ] Revisit docs-only patch versions and a type/interface/world inventory for memberless
      definitions.

## After 0.1

- [ ] JSON Schema emitter.
- [ ] Generated conversions between contract versions (for hosts that support several versions
      side by side).
- [ ] Replacing the lockfile with in-source numbers if the WIT annotations proposal
      (component-model issue 695) lands with a suitable annotation.
- [ ] Wider WIT coverage (maps, resources as opaque handles) only on demand.
- [ ] 1.0 (about 26 to 45 weeks in total): stable library API, multiple emitters, conformance
      suite.
