# witforge roadmap

witforge is built for its first consumer, the alint plugin contract, and its scope stays limited
to what that contract needs until the contract's first version ships. Estimates are
engineer-weeks for one experienced Rust engineer.

## 0.0.x: foundations (in progress)

- [x] Load a WIT package with every feature enabled (`@unstable` items kept).
- [x] Restricted-subset checker with a refuse-unknown catch-all.
- [x] Append-only wire lockfile: parse, render, reconcile, drift check, append-only check
      against an older lockfile.
- [x] `witforge check` and `witforge lock`.
- [x] CI: fmt, pedantic Clippy, rustdoc, tests, and the MSRV 1.88 build (the `wit-parser` floor),
      on a self-hosted Linux runner while the repository is private ([ci/README.md](ci/README.md)).
- [x] Rust edition 2024.
- [ ] Lockfile, as decided on 2026-10-24 (design overview section 5.2): function method ids and
      parameters, each entry's member type, one lockfile per version directory with
      `check`/`lock --against <previous wire.lock>`, and the draft rule.

## 0.1: the MVP alint needs (about 13 to 20 weeks in total)

- [ ] Internal model: resolved types, docs, and stability, independent of `wit-parser`'s types,
      so upstream churn is absorbed in one place.
- [ ] Rust emitter: plain, exhaustive types (structs, enums, `Option`, `Result`, `Vec`) in one
      module per version directory, traits for interfaces, docs carried from `///` comments.
- [ ] Protobuf emitter: messages and enums numbered from `wire.lock`, `reserved` for removed
      members, wrappers for nested `list` and `option`, `oneof` for variants and `result`, closed
      enums with a zero `UNSPECIFIED` value. Function request and response messages, and a
      method-id enum per interface.
- [ ] Protobuf glue: conversions between the plain types and `prost`-generated types (built with
      `protox` and `pbjson-build`), carrying every decode rule.
- [ ] `--check` mode for generated outputs (stale output fails CI); LF-only, deterministic output.
- [ ] Golden tests for every mapping row and round-trip tests through the protobuf glue (Rust
      value to protobuf to Rust value; proto3-JSON and back).
- [ ] CI guard that a released version's lockfile never changes against the merge base, and
      documentation for running `buf breaking` on the emitted schema as an optional extra check.
- [ ] Public repository and the first crates.io release once alint's contract `0.x` consumes it;
      CI moves back to GitHub-hosted Linux, macOS, and Windows runners, and the self-hosted
      runner is torn down.

## After 0.1

- [ ] JSON Schema emitter.
- [ ] Generated conversions between contract versions (for hosts that support several versions
      side by side).
- [ ] Replacing the lockfile with in-source numbers if the WIT annotations proposal
      (component-model issue 695) lands with a suitable annotation.
- [ ] Wider WIT coverage (maps, resources as opaque handles) only on demand.
- [ ] 1.0 (about 26 to 45 weeks in total): stable library API, multiple emitters, conformance
      suite.
