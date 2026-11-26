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
| `fixtures/emit/json/all.schema.json` | every JSON Schema mapping row—including all four `result` forms (`result<T, E>`, `result<T>`, `result<_, E>`, and bare `result`), member-level `description` fields, and integer/`f32` bounds: the golden Draft 7 output for `fixtures/emit/rust/all/contract.wit`; `tests/emit_json_schema.rs` compiles and validates instances against both Draft 7 and opt-in Draft 2020-12 |

## Emitter options, IR, and tooling surface

- **WIT subset expansion (`char`, `list<T, N>`, `map<K, V>`, `0.9.0`):**
  `tests/subset_expansion.rs` validates model lowering, canonical lockfile grammar, `inspect` IR,
  `diff` compatibility/defaulting, `WF1021`/`WF1003` refusals, Draft 7 and Draft 2020-12 JSON
  Schema validation, `--serde` large-array (`N > 32`) helpers, `convert.rs`, `prost` + `pbjson` +
  `glue` round-trips and error variants (`InvalidChar`, `InvalidLength`, `IntegerOutOfRange`,
  `MissingField`) in Editions 2021 and 2024, and `#![no_std]` + `alloc` (`prost.btree_map`)
  compilation.
- **Markdown reference emitter (`witforge doc` / `emit::doc`, `0.9.0`):** `tests/emit_doc.rs` and
  `crates/witforge-cli/tests/config.rs` verify deterministic `index.md` + per-version Markdown pages,
  changelog tables, wire numbers, stability/deprecation badges, `deps/` sections, Markdown/HTML
  sanitization, ownership headers, and `--check`/`--diff` parity.
- **Opt-in `--eq-hash` and `--try-from` (`0.9.0`):** `tests/eq_hash_try_from.rs` verifies
  transitive float-free `Eq`/`Hash` derives (including per-major scoped `deps/` and `--serde`),
  `TryFrom<&From>`/`TryFrom<From>` impls in `convert.rs` (including nominal types named `value`
  and `#![no_std]` + `alloc`), and Edition 2021/2024 `rustfmt`/Clippy/`missing_docs` parity.
- **Git-ref diff (`witforge diff --git-base`, `0.9.0`):** `crates/witforge-cli/tests/diff.rs`
  verifies safe Git object materialization, sibling `released` validation, symlink/submodule and
  shallow-clone guards (`WF9004`), deterministic `<rev>:<path>` redaction, and `witforge.toml`
  root/policy integration.
- **Project policy gates (`[project.<name>.policy]`, `0.9.0`):**
  `crates/witforge-cli/tests/config.rs` verifies `policy.diff-check` and
  `policy.forbid-unstable-in-release` (including CLI overrides, upward discovery across relative
  roots/symlinks, and fail-closed validation).
- **Unreleased dependency sync (`witforge deps sync`, `0.9.0`):**
  `crates/witforge-cli/tests/deps.rs` verifies major-scoped synchronization across unreleased
  directories, `--from`, `--check`, released-directory immutability, symlink/special-file refusal,
  and collision-safe atomic replacement.
- **Opt-in `no_std` + `alloc` (`0.8.0`):** `tests/emit_rust.rs` and `tests/emit_glue.rs` verify
  `RustOptions::new().with_no_std(true)` and `GlueOptions::new().with_no_std(true)` emit zero
  `::std::` references across plain modules, `convert.rs`, `worlds.rs`, `witforge_deps.rs`, and
  glue, and compile under `#![no_std]` + `extern crate alloc;`.
- **JSON Schema Draft 2020-12 (`0.8.0`):** `tests/emit_json_schema.rs` validates
  `SchemaOptions::new().with_draft(JsonSchemaDraft::Draft202012)` output (`$defs`, `deprecated: true`,
  Draft 2020-12 `$schema`, and `$ref` resolution across `deps/`) against `jsonschema`.
- **Contract IR (`inspect`, `0.8.0`):** `tests/inspect.rs` validates deterministic versioned JSON
  IR (`IR_VERSION = 1`) for single versions, multi-version roots, dependencies, stability metadata
  (including `@deprecated`), reserved lockfile entries, and order fingerprints.
- **`witforge-build` (`0.8.0`):** `crates/witforge-build/tests/compile.rs` compiles standalone and
  dependency `.proto` outputs with `protox` + `prost-build` and (under `--all-features`)
  `pbjson-build` round-trips.
- **Static `musl` archives (`0.8.0`):** CI's `musl` job and `release.yml` verify zero `INTERP` /
  `NEEDED` dynamic dependencies on `x86_64-unknown-linux-musl` and `aarch64-unknown-linux-musl`.

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
