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
| crate root | `Contract`, `ContractError`, `DiagnosticError`, `load`, `is_contract_error`, `error_diagnostics`, `error_located_diagnostics`, `ensure_same_dependency_graph`, `RELEASED_MANIFEST`, `ReleasedManifest`, `validate_retirement_order`, `discover_version_directories`, `ensure_no_symlinks_in_dir`, `default_released_manifest`, `ensure_version_directory`, `ensure_directory_version`, `load_released_manifest`, `load_root_released_manifest`, `ensure_not_released`, `is_released`, `ensure_lockfile_not_released` |
| `model` | `Version`, `Stability`, `QualifiedName`, `TypeExpr`, `ResultExpr`, `Field`, `Case`, `Flag`, `TypeDef`, `TypeDefKind`, `Param`, `Function`, `Interface`, `WorldInterface`, `World`, `Package`, `Dependency`, `Model`, `MemberKind`, `Member`, `build`, `members`, and their documented constructors, lookups, canonicalization, and member extraction methods |
| `lock` | `HEADER`, `ORDER_HEADER`, `PackageName`, `MemberKey`, `Entry`, `WireLock`, `LockMode`, `Changes`, `Discrepancy`, `Reconciliation`, and their documented constructors, parsing, reconciliation, validation, inspection, rendering, and writing methods |
| `emit::rust` | `HEADER`, `plan`, `plan_with_serde`, `plan_with_released`, `generate`, `generate_with_serde`, `check`, `check_with_serde`, `version_module_name`, `emit_version`, `emit_version_with_serde` |
| `emit::proto` | `HEADER`, `plan`, `plan_with_released`, `generate`, `check`, `emit_major` |
| `emit::json_schema` | `HEADER`, `plan`, `plan_with_released`, `generate`, `check`, `emit_version` |
| `emit::glue` | `HEADER`, `plan`, `plan_with_released`, `generate`, `check`, `validate_module_root`, `emit_root`, `emit_version` |
| `emit::output` | `GeneratedFiles` and its documented methods, `ensure_outside_wit_root`, `ensure_outside_released`, `ensure_distinct_output_roots`, `resolve_output_root` |
| `diff` | `Endpoint`, `Change`, `ChangeKind`, `Summary`, `DiffReport`, `ConversionImpact`, `ConversionOmission`, `DecodeVerdict`, `DecodeSummary`, `DiffAnalysis`, `FullDiff`, `compare`, `compare_with_impacts`, `compare_omissions`, `compare_with_locations`, `compare_with_omissions`, `compare_full` |
| `diagnostic` | `codes` and its constants, `CodeInfo`, `CATALOG`, `lookup`, `explanation`, `HELP_BASE_URL`, `help_uri`, `Diagnostic`, `LocatedDiagnostic`, `RelatedLocation`, `Position`, `Severity`, and their documented constructors and builders |
| `freeze` | `FreezeOptions`, `FreezeReport`, `check_root` |
| `subset` | `Violation`, `check`, `check_diagnostics`, `check_located_diagnostics` |

The exact line-oriented inventory is
[`public-api-inventory.txt`](public-api-inventory.txt). It records every public module, type,
variant, field, function, associated item, explicit trait implementation, and
`#[non_exhaustive]` promise. `ci/check-public-api.sh` regenerates it from rustdoc JSON under the
pinned `nightly-2026-09-11` toolchain and fails on either an undocumented addition or a removal.
The module table above is the human map; the checked inventory is the exhaustive surface.

## Data type evolution and construction

Every public struct and enum in `model`, `lock`, and `diagnostic` is `#[non_exhaustive]`.
Downstream matches therefore require a wildcard arm, and downstream code constructs public
model values, lockfile inputs, and diagnostic locations through the documented `new` constructors,
`Default`, parsers, or builders rather than struct literals. Their public fields remain readable
and mutable. Adding a field or variant is compatible within 1.x; changing or removing an existing
field or variant remains breaking.

The explicit trait implementations in the checked inventory are promised API, including derived
`Debug`, `Clone`, `Copy`, equality, ordering, hashing, and `Default` implementations. Adding a
constructor does not make construction part of the compiler pipeline: `model::build` remains the
normal way to obtain a model from WIT.

The `diff` report types are deliberately fixed exhaustive values for 1.0. Adding a field to
`ConversionImpact`, `DiffAnalysis`, or another listed diff value is a new type or a major-version
change, not an additive minor change.

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

## Errors

The boundary error type is `anyhow::Error`. Contract, lockfile, and generated-output violations
are classified by the stable [`ContractError`](../../crates/witforge/src/lib.rs) marker and
`is_contract_error`; I/O, parse, usage, and internal failures are not classified as contract
errors. A typed error enum is not promised for 1.0: replacing `anyhow` at every boundary would
change every public signature without improving the stable classification callers already use.
If a typed hierarchy is introduced later, it must be additive at the classification boundary and
must not change the meaning of `is_contract_error` within 1.x.

## Generated output

The Rust, protobuf, JSON Schema, and glue emitters are covered by [`emitters.md`](emitters.md);
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
| `witforge rust [<WIT_ROOT>]` | Emit or check plain Rust types, traits, `convert.rs`, and `worlds.rs` | `--out`, `--check`, `--diff`, `--serde`, `--no-serde`, `--released`, `--config`, `--no-config`, `--project` |
| `witforge proto [<WIT_ROOT>]` | Emit or check protobuf schemas | `--out`, `--check`, `--diff`, `--released`, `--config`, `--no-config`, `--project` |
| `witforge schema [<WIT_ROOT>]` | Emit or check Draft 7 JSON Schema files | `--out`, `--check`, `--diff`, `--released`, `--config`, `--no-config`, `--project` |
| `witforge glue [<WIT_ROOT>]` | Emit or check Rust/prost glue | `--out`, `--plain-root`, `--proto-root`, `--check`, `--diff`, `--released`, `--config`, `--no-config`, `--project` |
| `witforge generate` | Run every configured generator across the selected project or all projects | `--check`, `--diff`, `--serde`, `--no-serde`, `--released`, `--config`, `--no-config`, `--project` |
| `witforge new-version [<WIT_ROOT>]` | Create the next minor (or `--major`) version directory from the latest version | `--major`, `--config`, `--no-config`, `--project` |
| `witforge release <WIT_DIR>` | Validate a version directory and append it to `released` | `--released`, `--config`, `--no-config`, `--project` |
| `witforge diff <OLD> <NEW>` | Compare two version directories or WIT roots | `--json`, `--check[=VIEWS]` |
| `witforge freeze [<WIT_ROOT>]` | Verify released directories have not changed in Git history | `--base`, `--config`, `--no-config`, `--project` |
| `witforge completions <SHELL>` | Print shell completions for `bash`, `zsh`, or `fish` | — |
| `witforge explain <CODE>` | Print the catalogue explanation and documentation URL for a diagnostic code | — |

Config-aware commands discover the nearest `witforge.toml` upwards (stopping at `.git` or
filesystem root) when required paths are omitted; `--no-config` disables discovery.

## MSRV and platforms

The supported minimum Rust version is **1.88**. CI tests Linux, macOS, and Windows on stable Rust,
plus the 1.88 MSRV job. `cargo fmt --check`, Clippy with `-D warnings`, rustdoc with
`-D warnings`, and the full test suite are release gates.
