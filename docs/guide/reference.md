---
layout: default
title: CLI and configuration reference
---

# CLI and configuration reference

This page lists every `witforge` subcommand, flag, `witforge.toml` configuration key, and exit
code. The test suite verifies this page against the CLI parser so no option can go undocumented.

## Exit codes

| Exit code | Meaning |
|---|---|
| `0` | Success (valid contract, up-to-date generated files, passing gate) |
| `1` | Contract, lockfile, compatibility, release-freeze, or generated-output drift violation |
| `2` | Operational, CLI usage, parse, I/O, or safety-guard error |

## Global options

Every subcommand accepts these global options:

| Option | Description |
|---|---|
| `--format <FORMAT>` | Diagnostic output format: `human` (default), `json` (versioned diagnostic envelope on stdout), or `sarif` (SARIF 2.1.0 on stdout). |
| `-q`, `--quiet` | Suppress human notes and summaries (never hides errors, exit codes, or JSON/SARIF output). |
| `-h`, `--help` | Print help for `witforge` or a subcommand. |
| `-V`, `--version` | Print version (bare `X.Y.Z` on exact clean release tags/tarballs; appends short Git commit and `-dirty` on development builds). |

## Commands

### `witforge check`

Validate one version directory, an entire WIT root, or configured project(s) without writing.

| Option / Argument | Description |
|---|---|
| `[PATH]` | Version directory or WIT root; omit to check configured project root(s) via `witforge.toml`. |
| `--lock <PATH>` | Lockfile path (single version directory only; defaults to `<WIT_DIR>/wire.lock`). |
| `--first` | Treat every existing lockfile entry as a draft (single version directory only; conflicts with `--against`). |
| `--against <PATH>` | Treat entries from this baseline lockfile as released (single version directory only; conflicts with `--first`). |
| `--released <PATH>` | Released-version manifest path (defaults to `<WIT_DIR>/../released` or `<WIT_ROOT>/released`). |
| `--config <PATH>` | Explicit `witforge.toml` path (conflicts with `--no-config`). |
| `--no-config` | Disable upward `witforge.toml` discovery (conflicts with `--config`). |
| `--project <NAME>` | Project name in `witforge.toml`. |

### `witforge lock`

Reconcile `wire.lock` for one version directory, assigning wire numbers to new members and
reserving removed ones. Refuses released directories.

| Option / Argument | Description |
|---|---|
| `<WIT_DIR>` | WIT version directory to lock. |
| `--lock <PATH>` | Lockfile path (defaults to `<WIT_DIR>/wire.lock`). |
| `--first` | Create or reconcile a fresh draft lockfile with no baseline (conflicts with `--against`). |
| `--against <PATH>` | Reconcile against a predecessor version's `wire.lock` (conflicts with `--first`). |
| `--released <PATH>` | Additional released-version manifest to check alongside defaults. |

### `witforge rust`

Generate or check plain Rust types, interface traits, `worlds.rs`, `witforge_deps.rs`, and
`convert.rs`.

| Option / Argument | Description |
|---|---|
| `[WIT_ROOT]` | WIT root containing `v<major>.<minor>` version directories. |
| `--out <DIR>` | Output directory for generated Rust files. |
| `--check` | Verify existing files match without writing; exits `1` on drift. |
| `--diff` | Print a unified diff of output changes without changing the exit status. |
| `--serde` | Enable `serde::Serialize` and `serde::Deserialize` derives (conflicts with `--no-serde`). |
| `--no-serde` | Disable Serde derives, overriding `witforge.toml` (conflicts with `--serde`). |
| `--released <PATH>` | Released-version manifest path (defaults to `<WIT_ROOT>/released`). |
| `--config <PATH>` | Explicit `witforge.toml` path (conflicts with `--no-config`). |
| `--no-config` | Disable upward `witforge.toml` discovery (conflicts with `--config`). |
| `--project <NAME>` | Project name in `witforge.toml`. |

### `witforge proto`

Generate or check per-major protobuf schemas (`v<major>.proto` and dependency `.proto` files).

| Option / Argument | Description |
|---|---|
| `[WIT_ROOT]` | WIT root containing `v<major>.<minor>` version directories. |
| `--out <DIR>` | Output directory for generated `.proto` files. |
| `--check` | Verify existing files match without writing; exits `1` on drift. |
| `--diff` | Print a unified diff of output changes without changing the exit status. |
| `--released <PATH>` | Released-version manifest path (defaults to `<WIT_ROOT>/released`). |
| `--config <PATH>` | Explicit `witforge.toml` path (conflicts with `--no-config`). |
| `--no-config` | Disable upward `witforge.toml` discovery (conflicts with `--config`). |
| `--project <NAME>` | Project name in `witforge.toml`. |

### `witforge schema`

Generate or check Draft 7 JSON Schema files (`v<major>_<minor>.schema.json` and dependency
schemas).

| Option / Argument | Description |
|---|---|
| `[WIT_ROOT]` | WIT root containing `v<major>.<minor>` version directories. |
| `--out <DIR>` | Output directory for generated JSON Schema files. |
| `--check` | Verify existing files match without writing; exits `1` on drift. |
| `--diff` | Print a unified diff of output changes without changing the exit status. |
| `--released <PATH>` | Released-version manifest path (defaults to `<WIT_ROOT>/released`). |
| `--config <PATH>` | Explicit `witforge.toml` path (conflicts with `--no-config`). |
| `--no-config` | Disable upward `witforge.toml` discovery (conflicts with `--config`). |
| `--project <NAME>` | Project name in `witforge.toml`. |

### `witforge glue`

Generate or check Rust/prost conversion glue (`v<major>.rs`, `mod.rs`, `witforge_deps.rs`).

| Option / Argument | Description |
|---|---|
| `[WIT_ROOT]` | WIT root containing `v<major>.<minor>` version directories. |
| `--out <DIR>` | Output directory for generated glue files. |
| `--plain-root <PATH>` | Absolute Rust module path to generated plain modules (for example `crate::plain`). |
| `--proto-root <PATH>` | Absolute Rust module path to prost modules (for example `crate::proto`). |
| `--check` | Verify existing files match without writing; exits `1` on drift. |
| `--diff` | Print a unified diff of output changes without changing the exit status. |
| `--released <PATH>` | Released-version manifest path (defaults to `<WIT_ROOT>/released`). |
| `--config <PATH>` | Explicit `witforge.toml` path (conflicts with `--no-config`). |
| `--no-config` | Disable upward `witforge.toml` discovery (conflicts with `--config`). |
| `--project <NAME>` | Project name in `witforge.toml`. |

### `witforge generate`

Run every generator configured in `witforge.toml` across all projects or `--project`.

| Option | Description |
|---|---|
| `--check` | Verify all configured outputs without writing; exits `1` on drift. |
| `--diff` | Print unified diffs of output changes without changing the exit status. |
| `--serde` | Enable Serde derives across configured Rust outputs (conflicts with `--no-serde`). |
| `--no-serde` | Disable Serde derives across configured Rust outputs (conflicts with `--serde`). |
| `--released <PATH>` | Override the released-version manifest path for the run. |
| `--config <PATH>` | Explicit `witforge.toml` path (conflicts with `--no-config`). |
| `--no-config` | Disable upward `witforge.toml` discovery (conflicts with `--config`). |
| `--project <NAME>` | Select a single project from `witforge.toml`. |

### `witforge new-version`

Scaffold the next minor (or major) version directory from the latest existing version.

| Option / Argument | Description |
|---|---|
| `[WIT_ROOT]` | WIT root containing version directories; omit to use the configured project root. |
| `--major` | Create `v<major+1>.0` without copying `wire.lock` instead of `v<major>.<minor+1>`. |
| `--config <PATH>` | Explicit `witforge.toml` path (conflicts with `--no-config`). |
| `--no-config` | Disable upward `witforge.toml` discovery (conflicts with `--config`). |
| `--project <NAME>` | Select a single project from `witforge.toml`. |

### `witforge release`

Validate a version directory and append it to the `released` manifest.

| Option / Argument | Description |
|---|---|
| `<WIT_DIR>` | WIT version directory to validate and record as released. |
| `--released <PATH>` | Released-version manifest path (defaults to `<WIT_DIR>/../released`). |
| `--config <PATH>` | Explicit `witforge.toml` path (conflicts with `--no-config`). |
| `--no-config` | Disable upward `witforge.toml` discovery (conflicts with `--config`). |
| `--project <NAME>` | Select a single project from `witforge.toml`. |

### `witforge diff`

Compare two version directories or WIT roots and classify wire, source, API, and protobuf binary
decode compatibility.

| Option / Argument | Description |
|---|---|
| `<OLD>` | Older version directory or WIT root. |
| `<NEW>` | Newer version directory or WIT root. |
| `--json` | Emit the full diff report as JSON on stdout (takes precedence over `--format`). |
| `--check[=VIEWS]` | Exit `1` when any breaking change hits the selected views: comma-separated subset of `wire`, `source`, `api`, `decode` (bare `--check` defaults to `wire,source,api`, excluding `decode`). |

### `witforge freeze`

Verify that every directory listed in `wit/released` has remained immutable in Git history
(allowing only one-way `live -> retired` transitions).

| Option / Argument | Description |
|---|---|
| `[WIT_ROOT]` | WIT root containing `released` and version directories; omit to check all configured projects. |
| `--released <PATH>` | Released-version manifest path (defaults to `<WIT_ROOT>/released` or project `released` in `witforge.toml`). |
| `--base <REV>` | Lower-bound Git revision to bound the manifest history scan (defaults to full history). |
| `--config <PATH>` | Explicit `witforge.toml` path (conflicts with `--no-config`). |
| `--no-config` | Disable upward `witforge.toml` discovery (conflicts with `--config`). |
| `--project <NAME>` | Select a single project from `witforge.toml`. |

### `witforge completions`

Print shell completion scripts generated from the CLI command definition.

| Argument | Description |
|---|---|
| `<SHELL>` | Target shell: `bash`, `zsh`, or `fish`. |

### `witforge explain`

Print the catalogue explanation and public documentation URL for a diagnostic code.

| Argument | Description |
|---|---|
| `<CODE>` | Diagnostic code such as `WF1015` (case-insensitive). |

## `witforge.toml` configuration keys

`witforge.toml` is discovered upwards from the working directory (stopping at `.git` or the
filesystem root) unless `--config` or `--no-config` is passed. Unknown keys are refused (`exit 2`),
files must be UTF-8 without a BOM, and paths are resolved relative to `witforge.toml` without shell
expansion.

| Key | Required | Type | Description |
|---|---|---|---|
| `version` | yes | integer | Configuration schema version; must be `1`. |
| `[project.<name>]` | yes (at least one) | table | Named contract project table. |
| `project.<name>.root` | yes | path | WIT root directory containing `v<major>.<minor>` version directories. |
| `project.<name>.released` | no | path | Released-version manifest path (defaults to `<root>/released`). |
| `[project.<name>.rust]` | no | table | Plain Rust generator configuration. |
| `project.<name>.rust.out` | yes (if section set) | path | Output directory for generated Rust files. |
| `project.<name>.rust.serde` | no | boolean | Enable `serde::Serialize`/`Deserialize` derives (default `false`). |
| `[project.<name>.proto]` | no | table | Protobuf generator configuration. |
| `project.<name>.proto.out` | yes (if section set) | path | Output directory for generated `.proto` files. |
| `[project.<name>.schema]` | no | table | JSON Schema generator configuration. |
| `project.<name>.schema.out` | yes (if section set) | path | Output directory for generated Draft 7 JSON Schema files. |
| `[project.<name>.glue]` | no | table | Rust/prost glue generator configuration. |
| `project.<name>.glue.out` | yes (if section set) | path | Output directory for generated glue files. |
| `project.<name>.glue.plain-root` | yes (if section set) | string | Absolute Rust module path to generated plain modules (`crate::...` or `::ext::...`). |
| `project.<name>.glue.proto-root` | yes (if section set) | string | Absolute Rust module path to prost modules (`crate::...` or `::ext::...`). |
