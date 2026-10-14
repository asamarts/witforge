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
- [ ] CI on Linux, macOS, and Windows; MSRV 1.88 (the `wit-parser` floor).

## 0.1: the MVP alint needs (about 12 to 18 weeks in total)

- [ ] Internal model: resolved types, docs, and stability, independent of `wit-parser`'s types,
      so upstream churn is absorbed in one place.
- [ ] Rust emitter: plain types (structs, enums, `Option`, `Result`, `Vec`), traits for
      interfaces, docs carried from `///` comments.
- [ ] Protobuf emitter: messages and enums numbered from `wire.lock`, `reserved` for removed
      members, wrappers for nested `list` and `option`, `oneof` for variants and `result`, closed
      enums with a zero `UNSPECIFIED` value. Function request and response messages.
- [ ] `--check` mode for generated outputs (stale output fails CI); LF-only, deterministic output.
- [ ] Golden tests for every mapping row and round-trip tests (Rust value to protobuf to Rust
      value; proto3-JSON and back).
- [ ] `check --against <older lockfile>` for CI (append-only enforcement at the merge base), and
      documentation for running `buf breaking` at the WIRE_JSON level on the emitted schema.
- [ ] Explicit rename markers in doc comments, so a rename keeps its number.
- [ ] Public repository and the first crates.io release once alint's contract `0.x` consumes it.

## After 0.1

- [ ] JSON Schema emitter.
- [ ] Generated conversions between contract versions (for hosts that support several versions
      side by side).
- [ ] Replacing the lockfile with in-source numbers if the WIT annotations proposal
      (component-model issue 695) lands with a suitable annotation.
- [ ] Wider WIT coverage (maps, resources as opaque handles) only on demand.
- [ ] 1.0 (about 26 to 45 weeks in total): stable library API, multiple emitters, conformance
      suite.
