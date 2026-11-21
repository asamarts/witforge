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
| `fixtures/emit/json/all.schema.json` | every JSON Schema mapping row—including all four `result` forms (`result<T, E>`, `result<T>`, `result<_, E>`, and bare `result`) and integer/`f32` bounds: the golden Draft 7 output for `fixtures/emit/rust/all/contract.wit`, whose definitions `tests/emit_json_schema.rs` also compiles and validates instances against |

## Version and evolution surface

| Fixture | Covers |
|---|---|
| `fixtures/versions/wit` | two minors, lockfile continuity, released v0.1, order fingerprints, worlds, conversion output |
| `fixtures/versions/convert` | several types, variants, enums, flags, aliases, named structural types, and old-to-new/new-to-old conversions |
| `fixtures/deps-full/wit` | standard `deps/` types, aliases, lists, options, results, cross-dependency references, hyphenated package names, and generated Rust/protobuf/glue |
| `fixtures/deps-world/wit` | foreign world imports and exports, dependency glue, and a foreign value round-trip |
| `fixtures/deps-convert/wit` | version conversion with pinned foreign values |
| `fixtures/deps-unsupported/wit` | unsupported unexposed dependency items and a supported root; `tests/deps.rs` separately proves a required unsupported type is refused transitively |
| `fixtures/serde` | opt-in Serde derives and JSON round-trip |

## Gates

- `crates/witforge/tests/conformance.rs` checks the model coverage, world directions, lockfile
  member kinds, and every emitter against `fixtures/model/all.wit`.
- The golden and compile tests named in [`emitters.md`](emitters.md) run every emitter against
  the mapping fixtures.
- Generated output is checked by `ci/check-generated.sh` (which also compiles and round-trips
  the committed `examples/single` and `examples/evolved` outputs), and release directories by
  `ci/check-release-freeze.sh`.
- CI runs the suite on Linux, macOS, and Windows; MSRV is tested at 1.88.
- The 1.0 compatibility gate runs `ci/check-buf-history.sh` with buf 1.74: every fixture WIT
  root against every released generator at which the fixture exists. The historical release
  binary generates the old side; the candidate binary generates the new side. CI exposes the
  check through workflow dispatch, and the tag release workflow runs it before publication.
- `crates/witforge/tests/compile_matrix.rs` (backed by the shared compile-and-round-trip harness
  in `crates/witforge/tests/support/mod.rs`, described in [`quality.md`](quality.md)) runs the
  name matrix, shape matrix, fixed-seed property sample, and committed `examples/*` outputs
  through `prost-build` 0.14 + `pbjson-build` 0.9, `rustfmt`/`cargo fmt --check`,
  `cargo clippy -D warnings`, `-D missing_docs`, `cargo doc -D warnings`, `cargo test --doc`,
  and binary/JSON round-trips in both Edition 2021 and Edition 2024 under CI's `test`, `msrv`,
  and `generated` jobs (`W10-068`; see [`release-1.0.md`](release-1.0.md)).
