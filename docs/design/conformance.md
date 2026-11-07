# 1.0 conformance matrix

This document names the fixtures that prove the 1.0 supported WIT surface and the emitters that
consume it. Paths are relative to `crates/witforge/tests/`.

## Model and language surface

| Fixture | Covers |
|---|---|
| `fixtures/model/all.wit` | every primitive; record, variant, enum, flags; alias and `use` alias; named list/option/result; one-sided and bare result; nested list/option/result; functions with parameters and no result; docs and stability; escaped names; a world with one import and one export |

## Emitter mapping rows

| Fixture | Covers |
|---|---|
| `fixtures/emit/rust/all/contract.wit` | every Rust mapping row, including nested wrappers, aliases, named structural types, and a function with every parameter shape |
| `fixtures/emit/proto/mapping/contract.wit` | every protobuf mapping row, wrappers, oneofs, method ids, and reserved members |
| `fixtures/emit/glue/contract/contract.wit` | every glue type, request/response helper, presence rule, closed enum, and round-trip case |

## Version and evolution surface

| Fixture | Covers |
|---|---|
| `fixtures/versions/wit` | two minors, lockfile continuity, released v0.1, order fingerprints, worlds, conversion output |
| `fixtures/versions/convert` | several types, variants, enums, flags, aliases, named structural types, and old-to-new/new-to-old conversions |
| `fixtures/deps-full/wit` | standard `deps/` types, aliases, lists, options, results, cross-dependency references, hyphenated package names, and generated Rust/protobuf/glue |
| `fixtures/deps-world/wit` | foreign world imports and exports, dependency glue, and a foreign value round-trip |
| `fixtures/deps-convert/wit` | version conversion with pinned foreign values |
| `fixtures/deps-unsupported/wit` | unused unsupported dependency items and a supported root |
| `fixtures/serde` | opt-in Serde derives and JSON round-trip |

## Gates

- `crates/witforge/tests/conformance.rs` checks the model coverage, world directions, lockfile
  member kinds, and every emitter against `fixtures/model/all.wit`.
- The golden and compile tests named in [`emitters.md`](emitters.md) run every emitter against
  the mapping fixtures.
- Generated output is checked by `ci/check-generated.sh`, and release directories by
  `ci/check-release-freeze.sh`.
- CI runs the suite on Linux, macOS, and Windows; MSRV is tested at 1.88.
- The 1.0 compatibility check runs `ci/check-buf.sh` with buf 1.74 against the released tags
  (v0.1.2, v0.2.0, v0.3.0, v0.4.1) and the dependency fixtures; it is clean. The CI workflow keeps
  this as an optional workflow-dispatch job, while the release owner runs it before a release.
