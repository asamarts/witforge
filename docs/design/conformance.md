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

- **Opt-in protobuf file options (`W11-039`):** `tests/emit_proto.rs` and
  `crates/witforge-cli/tests/cli.rs` verify `ProtoOptions` / `ProtoPackageOptions`, CLI flags on
  `witforge proto`, `[project.<name>.proto]` / `[project.<name>.proto.packages.<proto_package>]`
  in `witforge.toml`, cross-file and same-file Java/Go/C# collision refusals (`WF1003`), option
  validation (`WF9001`), and real `protoc` compilation.
- **Cross-language protobuf conformance profile and fixture bundle (`W11-034`):**
  `witforge::conformance` (`CONFORMANCE_VERSION = 1`) and `witforge conformance --out <DIR>
  [--check] [--diff]` emit a self-contained deterministic conformance bundle (`wit/`, `proto/`,
  `vectors.json`) covering all 13 decode-profile rows, foreign `deps/` types and interface
  functions, 64-bit integer precision beyond `2^53 - 1`, supplementary-plane Unicode `char` and
  multi-byte UTF-8, `f32`/`f64` `NaN`/`Infinity`/`-Infinity`/`-0.0`, all four `result` shapes,
  nested wrapper state rules (`WitforgeOptionOf*` -> `InvalidState`, `WitforgeResultOf*` ->
  `UnknownCase`), and encode-side (`toProto`) boundary rejection vectors.
  `crates/witforge/tests/conformance_bundle.rs` proves every valid decode vector round-trips and
  every invalid decode vector fails with the exact expected `GlueError` kind and target through
  compiled Rust `prost` + `pbjson` + `witforge` glue in both binary protobuf and proto3-JSON forms.
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

## Cross-language protobuf conformance profile (`conformance_version = 1`)

`witforge::conformance` (`witforge conformance --out <DIR>`) emits a deterministic, versioned,
language-neutral conformance bundle that serves as the behavioral oracle for external `protoc`/`buf`
consumers and the built-in polyglot checked glue emitters (`M-p13`):

```text
<out>/
  vectors.json
  proto/
    v0.proto
    witforge.conformance_dep.v0.proto
  wit/
    v0.1/
      contract.wit
      wire.lock
      deps/conformance-dep/dep.wit
```

- **Default neutrality:** `ConformanceOptions::new()` (`witforge conformance --out <DIR>`) emits
  neutral `.proto` files with zero language options. Pass `--polyglot-proto-options` (or
  `ConformanceOptions::polyglot()` / explicit `--go-package`, `--java-package`,
  `--java-multiple-files`, `--java-outer-classname`, `--csharp-namespace`) to embed Go, Java, and
  C# file options via `ProtoOptions`.
- **`vectors.json` schema (`conformance_version = 1`):**
  - Line 1 carries `{"$comment":"@generated by witforge (conformance, witforge:conformance); do not edit.",`.
  - Top-level keys: `conformance_version` (`1`), `contract` (`package`, `wit_dir`, `proto_files`,
    `dependencies`), `error_kinds` (the 7 canonical error categories matching `GlueError`), and
    `vectors` (deterministic array of vector objects).
  - Each vector object carries:
    - `id` (unique kebab-case identifier) and `category` (`integers`, `scalars`, `presence`,
      `optionals`, `lists`, `fixed-lists`, `maps`, `enums`, `flags`, `variants`, `results`,
      `wrappers`, `deps`, `functions`, `encode-bounds`);
    - `direction`: `"decode"` (wire protobuf binary and proto3-JSON into plain WIT values, plus
      plain -> wire round-trip for `"accept"` vectors) or `"encode"` (invalid host-language plain
      values that checked `toProto` glue must reject before encoding);
    - `target_kind` (`"type"`, `"request"`, or `"response"`), canonical WIT `target`
      (for example `witforge:conformance/types/integers` or `witforge:conformance/api#notify`), and
      fully qualified `proto_message` (for example `.witforge.conformance.v0.TypesIntegers`);
    - `expected` (`"accept"` or `"reject"`), `error_kind` (`null` or one of `IntegerOutOfRange`,
      `MissingField`, `UnknownEnumValue`, `UnknownCase`, `InvalidState`, `InvalidChar`,
      `InvalidLength`), and `error_target` (`null` or the canonical package-relative WIT path
      matching Rust glue);
    - `proto_binary_base64` (RFC 4648 base64 protobuf wire bytes for `"decode"`, `null` for
      `"encode"`), `proto3_json` (canonical proto3-JSON string for `"decode"`, `null` for
      `"encode"`), `plain_json` (language-neutral plain value payload, encoding 64-bit integers as
      decimal strings so JSON parsers without 64-bit integers preserve exact precision), and
      `description`.
- **Decode and encode rules:**
  - Every `"decode"` `"accept"` vector must decode identically from both `proto_binary_base64` and
    `proto3_json`, re-encode through `toProto`, and round-trip losslessly (preserving `NaN`,
    `Infinity`, `-Infinity`, `-0.0` sign bit, `u64::MAX`, `i64::MIN`, `2^53 + 1`, supplementary
    `char` `U+1F680`, `U+0000`, `U+D7FF`, `U+E000`, `U+10FFFF`, and multi-byte UTF-8 with NUL;
    note that upstream `pbjson-build` 0.9 deserializes `"NaN"`/`"Infinity"`/`"-Infinity"` cleanly
    on `optional float`/`double` but serializes those three non-finite cases as `null`, so binary
    protobuf proves full round-trip on non-finite floats under `pbjson-build` 0.9).
  - Every `"decode"` `"reject"` vector must fail with the exact `error_kind` on binary protobuf; on
    proto3-JSON, either the proto3-JSON parser rejects unknown closed enum numbers during JSON
    deserialization or checked `fromProto` glue rejects with the exact `error_kind`.
  - Every `"encode"` `"reject"` vector must be rejected by checked `toProto` glue in host languages
    whose native types allow out-of-range integers, multi-scalar/empty/surrogate `char` strings or
    code points, wrong-length fixed lists, or invalid map keys.

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
