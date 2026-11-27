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
| `--no-std` | Emit `#![no_std]` + `alloc` compatible paths (`::alloc::*`, `::core::*`), overriding `witforge.toml` (conflicts with `--std`). |
| `--std` | Emit default `std` paths, overriding `no-std = true` in `witforge.toml` (conflicts with `--no-std`). |
| `--eq-hash` | Derive `Eq` and `Hash` on generated float-free records, variants, enums, and flags, overriding `witforge.toml` (conflicts with `--no-eq-hash`). |
| `--no-eq-hash` | Disable `Eq`/`Hash` derives, overriding `eq-hash = true` in `witforge.toml` (conflicts with `--eq-hash`). |
| `--try-from` | Emit `TryFrom` impls in `convert.rs` for converted nominal types, overriding `witforge.toml` (conflicts with `--no-try-from`). |
| `--no-try-from` | Disable `TryFrom` impls in `convert.rs`, overriding `try-from = true` in `witforge.toml` (conflicts with `--try-from`). |
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
| `--go-package <VALUE>` | Base `go_package` import path prefix (or explicit `path;alias` for single-file output), overriding `project.<name>.proto.go-package`. |
| `--java-package <VALUE>` | Base `java_package` prefix for emitted `.proto` files, overriding `project.<name>.proto.java-package`. |
| `--java-multiple-files` | Emit `option java_multiple_files = true;`, overriding config (conflicts with `--no-java-multiple-files`). |
| `--no-java-multiple-files` | Omit `option java_multiple_files`, overriding `project.<name>.proto.java-multiple-files` (conflicts with `--java-multiple-files`). |
| `--java-outer-classname <NAME>` | Emit `option java_outer_classname = "<NAME>";`, overriding `project.<name>.proto.java-outer-classname`. |
| `--csharp-namespace <VALUE>` | Base `csharp_namespace` prefix for emitted `.proto` files, overriding `project.<name>.proto.csharp-namespace`. |
| `--check` | Verify existing files match without writing; exits `1` on drift. |
| `--diff` | Print a unified diff of output changes without changing the exit status. |
| `--released <PATH>` | Released-version manifest path (defaults to `<WIT_ROOT>/released`). |
| `--config <PATH>` | Explicit `witforge.toml` path (conflicts with `--no-config`). |
| `--no-config` | Disable upward `witforge.toml` discovery (conflicts with `--config`). |
| `--project <NAME>` | Project name in `witforge.toml`. |

### `witforge schema`

Generate or check JSON Schema files (`v<major>_<minor>.schema.json` and dependency schemas;
Draft 7 by default, or Draft 2020-12 with `--draft 2020-12`).

| Option / Argument | Description |
|---|---|
| `[WIT_ROOT]` | WIT root containing `v<major>.<minor>` version directories. |
| `--out <DIR>` | Output directory for generated JSON Schema files. |
| `--draft <DRAFT>` | JSON Schema dialect: `7` (default) or `2020-12` (overrides `project.<name>.schema.draft`). |
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
| `--no-std` | Emit `#![no_std]` + `alloc` compatible paths (`::alloc::*`, `::core::*`), overriding `witforge.toml` (conflicts with `--std`). |
| `--std` | Emit default `std` paths, overriding `no-std = true` in `witforge.toml` (conflicts with `--no-std`). |
| `--check` | Verify existing files match without writing; exits `1` on drift. |
| `--diff` | Print a unified diff of output changes without changing the exit status. |
| `--released <PATH>` | Released-version manifest path (defaults to `<WIT_ROOT>/released`). |
| `--config <PATH>` | Explicit `witforge.toml` path (conflicts with `--no-config`). |
| `--no-config` | Disable upward `witforge.toml` discovery (conflicts with `--config`). |
| `--project <NAME>` | Project name in `witforge.toml`. |

### `witforge doc`

Generate or check deterministic Markdown contract reference pages (`index.md` and one
`v<major>_<minor>.md` per live version).

| Option / Argument | Description |
|---|---|
| `[WIT_ROOT]` | WIT root containing `v<major>.<minor>` version directories. |
| `--out <DIR>` | Output directory for generated Markdown reference files. |
| `--check` | Verify existing files match without writing; exits `1` on drift. |
| `--diff` | Print a unified diff of output changes without changing the exit status. |
| `--released <PATH>` | Released-version manifest path (defaults to `<WIT_ROOT>/released`). |
| `--config <PATH>` | Explicit `witforge.toml` path (conflicts with `--no-config`). |
| `--no-config` | Disable upward `witforge.toml` discovery (conflicts with `--config`). |
| `--project <NAME>` | Project name in `witforge.toml`. |

### `witforge conformance`

Generate or check the versioned cross-language protobuf conformance fixture bundle
(`conformance_version = 1`: `vectors.json`, `proto/`, and `wit/`).

| Option | Description |
|---|---|
| `--out <DIR>` | Output directory for the conformance bundle (`vectors.json`, `proto/`, `wit/`). |
| `--polyglot-proto-options` | Populate canonical Go, Java, and C# protobuf file options on emitted `.proto` files. |
| `--go-package <VALUE>` | Base `go_package` import path prefix for emitted `.proto` files. |
| `--java-package <VALUE>` | Base `java_package` prefix for emitted `.proto` files. |
| `--java-multiple-files` | Emit `option java_multiple_files = true;` on emitted `.proto` files (conflicts with `--no-java-multiple-files`). |
| `--no-java-multiple-files` | Omit `option java_multiple_files`, overriding `--polyglot-proto-options` (conflicts with `--java-multiple-files`). |
| `--java-outer-classname <NAME>` | Emit `option java_outer_classname = "<NAME>";` on emitted `.proto` files. |
| `--csharp-namespace <VALUE>` | Base `csharp_namespace` prefix for emitted `.proto` files. |
| `--check` | Verify existing bundle files match without writing; exits `1` on drift. |
| `--diff` | Print a unified diff of bundle changes without changing the exit status. |

### `witforge generate`

Run every generator configured in `witforge.toml` across all projects or `--project`.

| Option | Description |
|---|---|
| `--check` | Verify all configured outputs without writing; exits `1` on drift. |
| `--diff` | Print unified diffs of output changes without changing the exit status. |
| `--serde` | Enable Serde derives across configured Rust outputs (conflicts with `--no-serde`). |
| `--no-serde` | Disable Serde derives across configured Rust outputs (conflicts with `--serde`). |
| `--no-std` | Emit `#![no_std]` + `alloc` compatible paths across configured Rust and glue outputs (conflicts with `--std`). |
| `--std` | Emit default `std` paths across configured Rust and glue outputs, overriding config (conflicts with `--no-std`). |
| `--eq-hash` | Derive `Eq` and `Hash` on float-free generated Rust types, overriding config (conflicts with `--no-eq-hash`). |
| `--no-eq-hash` | Disable `Eq`/`Hash` derives on generated Rust types, overriding config (conflicts with `--eq-hash`). |
| `--try-from` | Emit `TryFrom` impls in `convert.rs` for converted nominal types, overriding config (conflicts with `--no-try-from`). |
| `--no-try-from` | Disable `TryFrom` impls in `convert.rs`, overriding config (conflicts with `--try-from`). |
| `--draft <DRAFT>` | Override the JSON Schema dialect for configured schema outputs (`7` or `2020-12`). |
| `--released <PATH>` | Override the released-version manifest path for the run. |
| `--config <PATH>` | Explicit `witforge.toml` path (conflicts with `--no-config`). |
| `--no-config` | Disable upward `witforge.toml` discovery (conflicts with `--config`). |
| `--project <NAME>` | Select a single project from `witforge.toml`. |

### `witforge init`

Scaffold a new witforge contract project (`wit/v0.1/contract.wit`, initial `wit/v0.1/wire.lock`,
and `witforge.toml`), refusing to overwrite any existing file.

| Option / Argument | Description |
|---|---|
| `[DIR]` | Target directory to initialize (defaults to `.`). |
| `--package <NS:NAME>` | WIT package identity (`<namespace>:<name>`, for example `example:hello`; defaults to `example:<dir-name>`). |
| `--serde` | Enable `serde = true` in the generated `witforge.toml` Rust section. |

### `witforge new-version`

Scaffold the next minor (or major) version directory from the latest existing version or an
explicit `--from` version.

| Option / Argument | Description |
|---|---|
| `[WIT_ROOT]` | WIT root containing version directories; omit to use the configured project root. |
| `--major` | Create `v<major+1>.0` without copying `wire.lock` instead of `v<major>.<minor+1>`. |
| `--from <VERSION>` | Source version directory to clone (for example `v0.2`), allowing maintenance minors on an earlier major line. |
| `--released <PATH>` | Path to the released-version manifest (defaults to `<WIT_ROOT>/released` or `project.<name>.released`). |
| `--config <PATH>` | Explicit `witforge.toml` path (conflicts with `--no-config`). |
| `--no-config` | Disable upward `witforge.toml` discovery (conflicts with `--config`). |
| `--project <NAME>` | Select a single project from `witforge.toml`. |

### `witforge deps`

Manage vendored `deps/` trees across unreleased version directories. Subcommands:

- `witforge deps sync`: synchronize `deps/` trees within each major without modifying released or retired directories.

### `witforge deps sync`

Synchronize vendored `deps/` trees from the canonical source version of each major into unreleased
version directories of the same major, refusing symlinks and never modifying released or retired
directories.

| Option / Argument | Description |
|---|---|
| `[WIT_ROOT]` | WIT root containing version directories; omit to use the configured project root. |
| `--from <VERSION>` | Canonical source version directory (for example `v0.1`); defaults to the highest released (or lowest live) version per major. |
| `--check` | Report `deps/` drift without writing; exits `1` (`WF1033`) if any unreleased version differs. |
| `--released <PATH>` | Path to the released-version manifest (defaults to `<WIT_ROOT>/released` or `project.<name>.released`). |
| `--config <PATH>` | Explicit `witforge.toml` path (conflicts with `--no-config`). |
| `--no-config` | Disable upward `witforge.toml` discovery (conflicts with `--config`). |
| `--project <NAME>` | Select a single project from `witforge.toml`. |

### `witforge release`

Validate a version directory and append it to the `released` manifest.

| Option / Argument | Description |
|---|---|
| `<WIT_DIR>` | WIT version directory to validate and record as released. |
| `--released <PATH>` | Released-version manifest path (defaults to `<WIT_DIR>/../released`). |
| `--forbid-unstable` | Refuse releasing a version containing any `@unstable` declaration, overriding config (conflicts with `--allow-unstable`). |
| `--allow-unstable` | Allow releasing a version containing `@unstable` declarations, overriding `policy.forbid-unstable-in-release = true` (conflicts with `--forbid-unstable`). |
| `--config <PATH>` | Explicit `witforge.toml` path (conflicts with `--no-config`). |
| `--no-config` | Disable upward `witforge.toml` discovery (conflicts with `--config`). |
| `--project <NAME>` | Select a single project from `witforge.toml`. |

### `witforge retire`

Mark a released version directory as `retired` in the `released` manifest, enforcing contiguous
oldest-prefix retirement order and requiring at least one remaining live version.

| Option / Argument | Description |
|---|---|
| `<WIT_DIR>` | Released WIT version directory to mark as `retired`. |
| `--released <PATH>` | Released-version manifest path (defaults to `<WIT_DIR>/../released`). |
| `--config <PATH>` | Explicit `witforge.toml` path (conflicts with `--no-config`). |
| `--no-config` | Disable upward `witforge.toml` discovery (conflicts with `--config`). |
| `--project <NAME>` | Select a single project from `witforge.toml`. |

### `witforge diff`

Compare two version directories or WIT roots (or compare a working-tree target against its Git
revision via `--git-base`) and classify wire, source, API, and protobuf binary decode
compatibility.

| Option / Argument | Description |
|---|---|
| `[OLD]` | Older version directory or WIT root (or target directory when `--git-base` is given; omit with `--git-base` to use the configured project root). |
| `[NEW]` | Newer version directory or WIT root (omit when `--git-base` is given). |
| `--git-base <REV>` | Compare the target version directory or WIT root against its committed Git tree at `<REV>`. |
| `--json` | Emit the full diff report as JSON on stdout (takes precedence over `--format`). |
| `--check[=VIEWS]` | Exit `1` when any breaking change hits the selected views: comma-separated subset of `wire`, `source`, `api`, `decode` (bare `--check` defaults to `[project.<name>.policy].diff-check` when configured, otherwise `wire,source,api` without `decode`). |
| `--config <PATH>` | Explicit `witforge.toml` path supplying `[project.<name>.policy].diff-check` and `root` for `--git-base` (conflicts with `--no-config`). |
| `--no-config` | Disable upward `witforge.toml` discovery (conflicts with `--config`). |
| `--project <NAME>` | Select a single project from `witforge.toml`. |

### `witforge inspect`

Inspect a single version directory or WIT root, printing a concise human summary or exporting the
deterministic, schema-versioned (`version: 1`) JSON contract IR with `--json`.

| Option / Argument | Description |
|---|---|
| `[PATH]` | Version directory or WIT root; omit to inspect the configured project root via `witforge.toml`. |
| `--json` | Emit the versioned JSON contract IR (`version: 1`) on stdout (takes precedence over `--format`). |
| `--released <PATH>` | Released-version manifest path when inspecting a WIT root (defaults to `<WIT_ROOT>/released`). |
| `--config <PATH>` | Explicit `witforge.toml` path (conflicts with `--no-config`). |
| `--no-config` | Disable upward `witforge.toml` discovery (conflicts with `--config`). |
| `--project <NAME>` | Select a single project from `witforge.toml`. |

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
| `project.<name>.rust.no-std` | no | boolean | Emit `#![no_std]` + `alloc` compatible paths (`::alloc::*`, `::core::*`) (default `false`). |
| `project.<name>.rust.eq-hash` | no | boolean | Derive `Eq` and `Hash` on generated float-free records, variants, enums, and flags (default `false`). |
| `project.<name>.rust.try-from` | no | boolean | Emit `TryFrom` impls in `convert.rs` for converted nominal types (default `false`). |
| `[project.<name>.proto]` | no | table | Protobuf generator configuration. |
| `project.<name>.proto.out` | yes (if section set) | path | Output directory for generated `.proto` files. |
| `project.<name>.proto.go-package` | no | string | Base `go_package` import path prefix (or explicit `path;alias` for single-file output). |
| `project.<name>.proto.java-package` | no | string | Base `java_package` prefix derived per emitted proto package. |
| `project.<name>.proto.java-multiple-files` | no | boolean | Emit `option java_multiple_files = true;` (or `false;`) on emitted `.proto` files. |
| `project.<name>.proto.java-outer-classname` | no | string | Emit `option java_outer_classname = "<NAME>";` (collision-checked per Java package and file). |
| `project.<name>.proto.csharp-namespace` | no | string | Base `csharp_namespace` prefix derived as PascalCase segments per emitted proto package. |
| `[project.<name>.proto.packages.<proto_package>]` | no | table | Exact per-package overrides (`go-package`, `java-package`, `java-multiple-files`, `java-outer-classname`, `csharp-namespace`) keyed by emitted proto package name. |
| `[project.<name>.schema]` | no | table | JSON Schema generator configuration. |
| `project.<name>.schema.out` | yes (if section set) | path | Output directory for generated JSON Schema files. |
| `project.<name>.schema.draft` | no | string | JSON Schema dialect: `"7"` (default) or `"2020-12"`. |
| `[project.<name>.glue]` | no | table | Rust/prost glue generator configuration. |
| `project.<name>.glue.out` | yes (if section set) | path | Output directory for generated glue files. |
| `project.<name>.glue.plain-root` | yes (if section set) | string | Absolute Rust module path to generated plain modules (`crate::...` or `::ext::...`). |
| `project.<name>.glue.proto-root` | yes (if section set) | string | Absolute Rust module path to prost modules (`crate::...` or `::ext::...`). |
| `project.<name>.glue.no-std` | no | boolean | Emit `#![no_std]` + `alloc` compatible paths (`::alloc::*`, `::core::*`) (default `false`). |
| `[project.<name>.doc]` | no | table | Markdown contract reference documentation generator configuration. |
| `project.<name>.doc.out` | yes (if section set) | path | Output directory for generated Markdown reference files. |
| `[project.<name>.policy]` | no | table | Project governance policy table. |
| `project.<name>.policy.diff-check` | no | array of strings | Default compatibility views for bare `witforge diff --check` (non-empty subset of `"wire"`, `"source"`, `"api"`, `"decode"`). |
| `project.<name>.policy.forbid-unstable-in-release` | no | boolean | Refuse `witforge release` when any `@unstable` declaration is present (default `false`). |
