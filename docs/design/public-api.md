# Public API stability (1.0)

Status: 1.0 API policy; no 1.0 release has been cut yet.

## Promise

The public surface is the modules and items below. Within 1.x:

- existing public items remain available with compatible behavior;
- removing or changing a public item requires a new major version;
- adding an enum variant or struct field is a breaking change, so additions are major-version
  work unless the item is explicitly documented as extensible;
- generated output and wire numbers have their own stronger rules in
  [`overview.md`](overview.md) and [`lockfile.md`](lockfile.md).

## Modules

| Module | Stable surface |
|---|---|
| crate root | `Contract`, `SourceFile`, `ContractError`, `DiagnosticError`, `load`, `load_sources`, `is_contract_error`, `error_diagnostics`, `error_located_diagnostics`, `ensure_same_dependency_graph`, `RELEASED_MANIFEST`, `ReleasedManifest`, `validate_retirement_order`, `discover_version_directories`, `ensure_no_symlinks_in_dir`, `default_released_manifest`, `ensure_version_directory`, `ensure_directory_version`, `load_released_manifest`, `load_root_released_manifest`, `ensure_not_released`, `is_released`, `ensure_lockfile_not_released` |
| `model` | `Version`, `Stability`, `QualifiedName`, `TypeExpr`, `ResultExpr`, `Field`, `Case`, `Flag`, `TypeDef`, `TypeDefKind`, `Param`, `Function`, `Interface`, `WorldInterface`, `World`, `Package`, `Dependency`, `Model`, `MemberKind`, `Member`, `build`, `members`, and their documented constructors, lookups, canonicalization, and member extraction methods |
| `lock` | `HEADER`, `ORDER_HEADER`, `PackageName`, `MemberKey`, `Entry`, `WireLock`, `LockMode`, `Changes`, `Discrepancy`, `Reconciliation`, and their documented constructors, parsing, reconciliation, validation, inspection, rendering, and writing methods |
| `emit::rust` | `HEADER`, `RustOptions`, `plan`, `plan_with_serde`, `plan_with_released`, `plan_with_options`, `generate`, `generate_with_serde`, `check`, `check_with_serde`, `version_module_name`, `emit_version`, `emit_version_with_serde`, `emit_version_with_options` |
| `emit::proto` | `HEADER`, `ProtoOptions`, `ProtoPackageOptions`, `plan`, `plan_with_released`, `plan_with_options`, `generate`, `generate_with_options`, `check`, `check_with_options`, `emit_major`, `emit_major_with_options` |
| `emit::json_schema` | `HEADER`, `JsonSchemaDraft`, `SchemaOptions`, `plan`, `plan_with_released`, `plan_with_options`, `generate`, `check`, `emit_version`, `emit_version_with_options` |
| `emit::glue` | `HEADER`, `GlueOptions`, `plan`, `plan_with_released`, `plan_with_options`, `generate`, `check`, `validate_module_root`, `emit_root`, `emit_root_with_options`, `emit_version`, `emit_version_with_options` |
| `emit::doc` | `HEADER`, `plan`, `plan_with_released`, `generate`, `check`, `emit_version`, `emit_index` |
| `emit::output` | `HeaderStyle`, `GeneratedFiles`, `GeneratedFileFinding`, `GeneratedFileStatus` and their documented methods (including additive nested-tree `new_nested`, `insert_relative`, `insert_marker`, `is_nested`, `markers`), `validate_relative_path`, `ensure_outside_wit_root`, `ensure_outside_released`, `ensure_distinct_output_roots`, `resolve_output_root` |
| `emit::polyglot` | `TargetLanguage`, case converters (`to_snake_case`, `to_upper_snake_case`, `to_pascal_case`, `to_camel_case`), keyword/escaping helpers for TypeScript/Python/Go/Java/C#, version/dependency path helpers, and `doc_lines` |
| `conformance` | `CONFORMANCE_VERSION`, `ConformanceOptions`, `ConformanceBundle`, `plan`, `generate`, `check`, `check_with_command`, `unified_diff`, `vectors_json` |
| `diff` | `Endpoint`, `Change`, `ChangeElement`, `ChangeKind`, `Summary`, `DiffReport`, `ConversionImpact`, `ConversionOmission`, `DecodeVerdict`, `DecodeSummary`, `DiffAnalysis`, `FullDiff`, `compare`, `compare_with_impacts`, `compare_omissions`, `compare_with_locations`, `compare_with_omissions`, `compare_full`, `compare_git_base` |
| `diagnostic` | `codes` and its constants, `CodeInfo`, `CATALOG`, `lookup`, `explanation`, `HELP_BASE_URL`, `help_uri`, `Diagnostic`, `LocatedDiagnostic`, `RelatedLocation`, `Position`, `Severity`, and their documented constructors and builders |
| `freeze` | `FreezeOptions`, `FreezeReport`, `check_root`, `check_root_with_manifest` |
| `inspect` | `IR_VERSION`, `emit_version_json`, `inspect_version_json`, `inspect_root_json`, `summarize_version`, `summarize_root` |
| `subset` | `Violation`, `check`, `check_diagnostics`, `check_located_diagnostics` |
| `workflow` | `CheckTargetKind`, `Baseline`, `VersionOptions`, `FindingKind`, `Finding`, `Report`, `LockReport`, `classify_check_target`, `check`, `check_root`, `check_release`, `lock` |

The exact line-oriented inventory is
[`public-api-inventory.txt`](public-api-inventory.txt). It records every public module, type,
variant, field and function signature, associated item, explicit trait implementation, and
`#[non_exhaustive]` promise. `ci/check-public-api.sh` regenerates it from rustdoc JSON under the
pinned `nightly-2026-09-11` toolchain. Before and on the 0.7.0 tag commit, the complete delta
from the checked 0.6.0 inventory must equal the reviewed pre-1.0 break inventory. On descendants
of `v0.7.0`, the newest published release tag at or after `v0.7.0` (excluding a tag on `HEAD`
itself) becomes the signature-aware baseline: every entry from that tagged release must remain
while additive entries are allowed. Cargo-semver-checks independently enforces Rust
semver against the newest published release. An undocumented addition, removal, signature/type
change, or extensibility change therefore fails. The module table above is the human map; the
checked inventory is the exhaustive surface.

## Data type evolution and construction

Every public struct and enum in `model`, `lock`, and `diagnostic` is `#[non_exhaustive]`.
Downstream matches therefore require a wildcard arm, and downstream code constructs public
model values, lockfile inputs, and diagnostic locations through the documented `new` constructors,
`Default`, parsers, or builders rather than struct literals. Their public fields remain readable
and mutable. Adding a field or variant is compatible within 1.x; changing or removing an existing
field or variant remains breaking.

The new `ChangeElement`, generated-file finding, `HeaderStyle`, `TargetLanguage`, `workflow`,
`ConformanceOptions`, `ConformanceBundle`, and emitter-option types
(`RustOptions`, `GlueOptions`, `SchemaOptions`, `JsonSchemaDraft`, `ProtoOptions`,
`ProtoPackageOptions`) are also `#[non_exhaustive]`;
their constructors and builders are the downstream construction boundary. `RustOptions`,
`GlueOptions`, `SchemaOptions`, `ProtoOptions`, `ProtoPackageOptions`, and `ConformanceOptions`
deliberately omit `Copy` so future additive options may carry non-`Copy` values without breaking
SemVer.

The explicit trait implementations in the checked inventory are promised API, including derived
`Debug`, `Clone`, `Copy`, equality, ordering, hashing, and `Default` implementations. Adding a
constructor does not make construction part of the compiler pipeline: `model::build` remains the
normal way to obtain a model from WIT.

`Stability::Deprecated` records its `@since` or unstable feature origin, WIT deprecation version,
and deterministic consumer note. It is an additive variant because `Stability` became
`#[non_exhaustive]` before this change.

The `diff` report types are deliberately fixed exhaustive values for 1.0.
`ChangeKind::StabilityChanged { from, to }` is the final pre-1.0 addition; 0.6 consumers matching
`ChangeKind` add that arm during the 0.7 migration. Adding another field or variant to
`ConversionImpact`, `DiffAnalysis`, `ChangeKind`, or another listed diff value after 1.0 is a
major-version change, not an additive minor change.

`diff::compare_with_impacts` returns the same `DiffReport` as `diff::compare` plus informational
`ConversionImpact` notes. The notes do not change compatibility counts or `--check` behavior.

`Contract` is opaque: its `wit-parser` fields are crate-private and cannot be constructed or
mutated by downstream code.

`WorldInterface` carries the interface name plus an optional dependency `Package`, and
`Model.dependencies` carries standard `deps/` packages. These fields are part of the 1.0 model
surface. For a root with dependencies, `emit::rust::plan`, `emit::proto::plan`,
`emit::json_schema::plan`, and `emit::glue::plan` are the complete entry points: they emit the
shared dependency files. The lower-level `emit_version`, `emit_major`, and `emit_root` functions
emit one local file and do not create the shared dependency files themselves.

## Companion crate: `witforge-build`

`witforge-build` is an optional `build-dependencies` helper crate (first published in `0.8.0`) that
wraps `protox`, `prost-build`, and optional `pbjson-build` (behind feature `pbjson`) for compiling
`witforge proto` output.

- **Public surface:** `DEFAULT_INCLUDE_FILE`, `Config`, `BuildOutput`, `Error` (`#[non_exhaustive]`),
  `discover_protos`, and `compile_protos`.
- **Direct 0.x third-party dependency exposure:** `witforge-build` intentionally exposes
  `protox::Error` (via `Error::Protox`), `&mut prost_build::Config` (via `Config::prost_config` /
  `Config::with_prost_config`), and `&mut pbjson_build::Builder` (via `Config::pbjson_builder` /
  `Config::with_pbjson_builder` under feature `pbjson`) so build scripts can customize prost/pbjson
  options directly. Because `protox`, `prost-build`, and `pbjson-build` are currently `0.x` crates,
  any upstream breaking bump of those crates is a SemVer-breaking change for `witforge-build`.
- **1.0 versioning boundary:** The `1.0` public API promise and `public-api-inventory.txt` gate in
  this document apply to the `witforge` compiler library crate. Before `witforge` cuts `1.0.0`,
  either `witforge-build` will decouple its version from the workspace lockstep (`0.x` until its
  underlying protobuf build dependencies reach `1.0`) or its public surface will be inventoried and
  gated separately so an upstream `prost-build`/`protox`/`pbjson-build` bump never forces a
  `witforge` `2.0.0`.

## Configuration boundary

The `witforge.toml` data model and upward configuration discovery are CLI-only. Publishing that
Rust representation would freeze a second schema beside the normative TOML format. Library
callers pass paths, baselines, released manifests, module roots, and emitter options directly to
`workflow` and `emit`; the `workflow` module documentation contains a compiling `build.rs`
example. Library functions return reports, findings, and classified errors; they never print or
exit.

## Errors

The boundary error type is `anyhow::Error`. Contract, lockfile, and generated-output violations
are classified by the stable [`ContractError`](../../crates/witforge/src/lib.rs) marker and
`is_contract_error`; I/O, parse, usage, and internal failures are not classified as contract
errors. A typed error enum is not promised for 1.0: replacing `anyhow` at every boundary would
change every public signature without improving the stable classification callers already use.
If a typed hierarchy is introduced later, it must be additive at the classification boundary and
must not change the meaning of `is_contract_error` within 1.x.

## Generated output

The Rust, protobuf, JSON Schema, glue, and Markdown documentation emitters are covered by
[`emitters.md`](emitters.md), and shared polyglot emitter conventions are covered by
[`polyglot-emitters.md`](polyglot-emitters.md);
the lockfile and frozen-version rules are in [`lockfile.md`](lockfile.md). Generated output is
part of the release contract: the conformance matrix and `--check` gates are in
[`conformance.md`](conformance.md).

`GlueError` is a type written into generated glue source. It is part of that generated-output
contract, not an item exported by the `witforge` library; there is no
`witforge::emit::glue::GlueError`.

## CLI commands

The stable 1.0 CLI surface (every command accepts `--format human|json|sarif` and `-q`/`--quiet`;
`--version` prints the bare version on an exact clean release tag or tarball build and adds the
short Git commit and `-dirty` suffix on development builds):

| Command | Purpose | Key options |
|---|---|---|
| `witforge check [<PATH>]` | Validate one version directory or an entire WIT root (or configured projects) | `--lock`, `--first`, `--against`, `--released`, `--config`, `--no-config`, `--project` |
| `witforge lock <WIT_DIR>` | Reconcile `wire.lock` for one version directory | `--lock`, `--first`, `--against`, `--released` |
| `witforge rust [<WIT_ROOT>]` | Emit or check plain Rust types, traits, `convert.rs`, and `worlds.rs` | `--out`, `--check`, `--diff`, `--serde`, `--no-serde`, `--no-std`, `--std`, `--eq-hash`, `--no-eq-hash`, `--try-from`, `--no-try-from`, `--released`, `--config`, `--no-config`, `--project` |
| `witforge proto [<WIT_ROOT>]` | Emit or check protobuf schemas | `--out`, `--go-package`, `--java-package`, `--java-multiple-files`, `--no-java-multiple-files`, `--java-outer-classname`, `--csharp-namespace`, `--check`, `--diff`, `--released`, `--config`, `--no-config`, `--project` |
| `witforge schema [<WIT_ROOT>]` | Emit or check JSON Schema files (Draft 7 default, opt-in Draft 2020-12) | `--out`, `--check`, `--diff`, `--draft`, `--released`, `--config`, `--no-config`, `--project` |
| `witforge glue [<WIT_ROOT>]` | Emit or check Rust/prost glue | `--out`, `--plain-root`, `--proto-root`, `--check`, `--diff`, `--no-std`, `--std`, `--released`, `--config`, `--no-config`, `--project` |
| `witforge doc [<WIT_ROOT>]` | Emit or check deterministic Markdown contract reference docs | `--out`, `--check`, `--diff`, `--released`, `--config`, `--no-config`, `--project` |
| `witforge conformance` | Emit or check the versioned cross-language protobuf conformance fixture bundle | `--out`, `--polyglot-proto-options`, `--go-package`, `--java-package`, `--java-multiple-files`, `--no-java-multiple-files`, `--java-outer-classname`, `--csharp-namespace`, `--check`, `--diff` |
| `witforge generate` | Run every configured generator across the selected project or all projects | `--check`, `--diff`, `--serde`, `--no-serde`, `--no-std`, `--std`, `--eq-hash`, `--no-eq-hash`, `--try-from`, `--no-try-from`, `--draft`, `--released`, `--config`, `--no-config`, `--project` |
| `witforge init [DIR]` | Scaffold a new contract workspace (`wit/v0.1/`, `wire.lock`, `witforge.toml`) | `--package`, `--serde` |
| `witforge new-version [<WIT_ROOT>]` | Create the next minor (or `--major`) version directory from the latest or `--from` version | `--major`, `--from`, `--released`, `--config`, `--no-config`, `--project` |
| `witforge deps sync [<WIT_ROOT>]` | Synchronize or check `deps/` across unreleased versions of each major | `--from`, `--check`, `--released`, `--config`, `--no-config`, `--project` |
| `witforge release <WIT_DIR>` | Validate a version directory and append it to `released` | `--forbid-unstable`, `--allow-unstable`, `--released`, `--config`, `--no-config`, `--project` |
| `witforge retire <WIT_DIR>` | Mark the oldest live released version `retired` in `released` | `--released`, `--config`, `--no-config`, `--project` |
| `witforge diff <OLD> <NEW>` / `witforge diff --git-base <REV> [<PATH>]` | Compare two version directories/roots or a working-tree target against a Git base revision | `--git-base`, `--json`, `--check[=VIEWS]`, `--config`, `--no-config`, `--project` |
| `witforge inspect [<PATH>]` | Emit versioned JSON contract IR (`--json`) or a human contract summary | `--json`, `--released`, `--config`, `--no-config`, `--project` |
| `witforge freeze [<WIT_ROOT>]` | Verify released directories have not changed in Git history | `--base`, `--released`, `--config`, `--no-config`, `--project` |
| `witforge completions <SHELL>` | Print shell completions for `bash`, `zsh`, or `fish` | — |
| `witforge explain <CODE>` | Print the catalogue explanation and documentation URL for a diagnostic code | — |

Config-aware commands discover the nearest `witforge.toml` upwards (stopping at `.git` or
filesystem root) when required paths are omitted; `--no-config` disables discovery.

## MSRV and platforms

The supported minimum Rust version is **1.88**. CI tests Linux, macOS, and Windows on stable Rust,
plus the 1.88 MSRV job. `cargo fmt --check`, Clippy with `-D warnings`, rustdoc with
`-D warnings`, and the full test suite are release gates.
