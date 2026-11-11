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
| crate root | `Contract`, `ContractError`, `load`, `is_contract_error`, `ReleasedManifest`, release-manifest helpers, version-directory and released-directory helpers |
| `model` | `Version`, `Stability`, `QualifiedName`, `TypeExpr`, `ResultExpr`, `Field`, `Case`, `Flag`, `TypeDef`, `TypeDefKind`, `Param`, `Function`, `Interface`, `WorldInterface`, `World`, `Package`, `Dependency`, `Model`, `MemberKind`, `Member`, `build`, and `members` |
| `lock` | `HEADER`, `ORDER_HEADER`, `PackageName`, `MemberKey`, `Entry`, `WireLock`, `LockMode`, `Changes`, and their documented methods |
| `emit::rust` | `HEADER`, `plan`, `plan_with_serde`, `generate`, `generate_with_serde`, `check`, `check_with_serde`, `version_module_name`, `emit_version`, `emit_version_with_serde` |
| `emit::proto` | `HEADER`, `plan`, `generate`, `check`, `emit_major` |
| `emit::json_schema` | `HEADER`, `plan`, `generate`, `check`, `emit_version` |
| `emit::glue` | `HEADER`, `plan`, `generate`, `check`, `validate_module_root`, `emit_root`, `emit_version` |
| `emit::output` | `GeneratedFiles`, `ensure_outside_wit_root`, `absolute` |
| `diff` | `Endpoint`, `Change`, `ChangeKind`, `Summary`, `DiffReport`, `compare` |
| `diagnostic` | `Diagnostic`, `Position`, `Severity` |
| `subset` | `Violation`, `check` |

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

## MSRV and platforms

The supported minimum Rust version is **1.88**. CI tests Linux, macOS, and Windows on stable Rust,
plus the 1.88 MSRV job. `cargo fmt --check`, Clippy with `-D warnings`, rustdoc with
`-D warnings`, and the full test suite are release gates.
