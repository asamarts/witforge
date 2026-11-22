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
| crate root | `Contract`, `ContractError`, `DiagnosticError`, `load`, `is_contract_error`, `error_diagnostics`, `error_located_diagnostics`, `ReleasedManifest`, `discover_version_directories`, `ensure_no_symlinks_in_dir`, release-manifest helpers, version-directory and released-directory helpers |
| `model` | `Version`, `Stability`, `QualifiedName`, `TypeExpr`, `ResultExpr`, `Field`, `Case`, `Flag`, `TypeDef`, `TypeDefKind`, `Param`, `Function`, `Interface`, `WorldInterface`, `World`, `Package`, `Dependency`, `Model`, `MemberKind`, `Member`, `build`, and `members` |
| `lock` | `HEADER`, `ORDER_HEADER`, `PackageName`, `MemberKey`, `Entry`, `WireLock`, `LockMode`, `Changes`, `Discrepancy`, `Reconciliation`, and their documented methods |
| `emit::rust` | `HEADER`, `plan`, `plan_with_serde`, `generate`, `generate_with_serde`, `check`, `check_with_serde`, `version_module_name`, `emit_version`, `emit_version_with_serde` |
| `emit::proto` | `HEADER`, `plan`, `generate`, `check`, `emit_major` |
| `emit::json_schema` | `HEADER`, `plan`, `generate`, `check`, `emit_version` |
| `emit::glue` | `HEADER`, `plan`, `generate`, `check`, `validate_module_root`, `emit_root`, `emit_version` |
| `emit::output` | `GeneratedFiles`, `ensure_outside_wit_root`, `ensure_outside_released`, `ensure_distinct_output_roots`, `resolve_output_root`, `absolute` |
| `diff` | `Endpoint`, `Change`, `ChangeKind`, `Summary`, `DiffReport`, `ConversionImpact`, `DiffAnalysis`, `compare`, `compare_with_impacts`, `compare_with_locations` |
| `diagnostic` | `codes`, `CodeInfo`, `CATALOG`, `lookup`, `HELP_BASE_URL`, `help_uri`, `Diagnostic`, `LocatedDiagnostic`, `RelatedLocation`, `Position`, `Severity` |
| `freeze` | `FreezeOptions`, `FreezeReport`, `check_root` |
| `subset` | `Violation`, `check`, `check_diagnostics`, `check_located_diagnostics` |

`diff::compare_with_impacts` returns the same `DiffReport` as `diff::compare` plus informational
`ConversionImpact` notes. The notes do not change compatibility counts or `--check` behavior.
`ConversionImpact` and `DiffAnalysis` are fixed value types for 1.0: adding a field is a new type
or a major-version change, not an additive minor change.

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

The stable 1.0 CLI surface (every command accepts `--format human|json|sarif`):

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
| `witforge diff <OLD> <NEW>` | Compare two version directories or WIT roots | `--json`, `--check` |
| `witforge freeze [<WIT_ROOT>]` | Verify released directories have not changed in Git history | `--base`, `--config`, `--no-config`, `--project` |

Config-aware commands discover the nearest `witforge.toml` upwards (stopping at `.git` or
filesystem root) when required paths are omitted; `--no-config` disables discovery.

## MSRV and platforms

The supported minimum Rust version is **1.88**. CI tests Linux, macOS, and Windows on stable Rust,
plus the 1.88 MSRV job. `cargo fmt --check`, Clippy with `-D warnings`, rustdoc with
`-D warnings`, and the full test suite are release gates.
