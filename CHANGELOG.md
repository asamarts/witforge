# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- `witforge::ensure_lockfile_not_released`, which `lock` uses to refuse a `--lock` path inside a
  released directory.
- A CI job that runs pinned actionlint and shellcheck over the workflows, with shellcheck's
  masked-return check enabled for every `run:` script, and shellcheck over `ci/`.
- `ci/check-release.sh` requires `witforge-cli`'s dependency on `witforge` to be the release
  version, since the CLI uses the library's API as of that version.

### Fixed

- Generation never writes through a symlink or over a file without witforge's header: such an
  expected path fails generation with exit `2` before anything is written or removed, and
  `--check` reports an expected file that is a symlink.
- `lock` refuses a `--lock` path inside a released directory, and a released version directory
  that is a symlink into a tree without a manifest: the manifest beside the directory as written
  applies as well as the one beside its resolved location.
- The generators include a version directory that is a symlink instead of skipping it, and refuse
  a version directory name with a sign or leading zeros (`v+0.1`, `v00.02`) instead of reading it
  as a version.
- Lockfile keys from another package are refused in every mode, not only with `--against`, and a
  parameter key's interface must be a WIT identifier.
- `lock` leaves a byte-identical lockfile untouched instead of rewriting it.
- `check` names the exact `witforge lock` command, with its mode and paths, in its hints.
- The release workflow fails when the release check or the crates.io lookup fails. Both ran
  inside `echo "...$(...)"`, which hid the failure, so a tag that did not match the version, or
  a crates.io error, did not stop the run.
- `ci/check-release-freeze.sh` reads the manifest's full history, merges included, and checks a
  released directory against every commit that listed it, so a merge can no longer hide the
  commit that released it. It resolves the WIT root from the current directory (spellings such
  as `./wit`, `wit/../wit`, or a path from a subdirectory were silently left unchecked), fails
  outside a git repository or for a missing root, and refuses untracked files in a released
  directory.
- `ci/test-publish.sh` reads the workspace version and tests with a version above any release,
  so the CLI is verified against the scratch copy of the library, not the published release.

### Documentation

- In the proto schema the wire numbers and reserved names are append-only, not the schema: a type
  or function removed as a whole drops its messages. The overview's evolution rules for a new
  minor are authoring policy that 0.1 does not enforce. Each generator needs its own output root.
  `ci/check-generated.sh` covers witforge's own fixtures; a consumer runs `--check` on its
  committed output.

## [0.1.0] - 2026-10-31

### Added

- WIT package loading with every feature enabled.
- The restricted-subset checker.
- The owned internal model with canonical types, source order, docs, stability, and
  lockfile-member extraction.
- The append-only v2 wire lockfile (`wire.lock`) with typed keys, member kinds, canonical types,
  functions, parameters, reservations, draft/release modes, and atomic writes.
- The clap-based `witforge check` and `witforge lock` commands with `--first`, `--against`, and
  `--released`, plus the v2 evolution fixture and property/CLI tests.
- Plain Rust types and interface traits, protobuf schemas with append-only wire numbers, and
  prost/pbjson glue with per-function request/response helpers and `METHOD` constants.
- `witforge rust`, `witforge proto`, and `witforge glue` commands, with deterministic output,
  ownership headers, stale-file cleanup, and `--check`; `proto` and `glue` generate each major
  from its highest minor.
- The representative alint plugin contract end-to-end proof, including compiled plain/glue output
  and binary/proto3-JSON round-trips.
- CI gates for generated output, released-directory freezes, append-only release manifests, and
  optional `buf breaking`.
- The 0.1 performance/ergonomics baseline and deferred-option triage.
- `witforge::ContractError` and `witforge::is_contract_error`, which tell a contract, lockfile,
  or output violation apart from a usage, I/O, or parse error, and
  `witforge::load_root_released_manifest` for a WIT root's manifest.
- A tag-driven release workflow that publishes through crates.io trusted publishing and creates
  the GitHub release from this changelog.

### Changed

- Workspace and crate metadata identify the 0.1.0 release.

### Fixed

- Refuse patch, pre-release, and build-metadata contract versions, which have no supported
  version-directory mapping.
- Refuse unknown WIT `Type` variants instead of silently accepting them.
- Refuse lockfile number assignment beyond protobuf's maximum field number.
- Keep `wit-parser` types out of the public `Contract` API.
- Accept `--help` on subcommands and add `--version`.
- Guard the CI runner cleanup trap against signaling its own process group.
- Run the full test suite in the MSRV CI job, not only a build.
- Glue named-option singular wrappers and canonical alias/structural names.
- Final-newline and generated-file-name ownership edge cases.
- Release-manifest dot-entry validation.
- Default-mode `lock` and `check` refuse a missing lockfile instead of starting an empty one, and
  `rust`, `proto`, and `glue` require each minor's lockfile to continue the previous minor's.
- `check`, `rust`, `proto`, and `glue` validate `--released` and the default manifest.
- `check` reports only the subset violations when there are any, instead of also reporting every
  locked member as removed.
- Every contract, lockfile, or output violation exits `1`, including those from `lock` and the
  generators; usage, I/O, and parse errors exit `2`.
- Files that are not UTF-8 text in an output root are ignored instead of failing generation.
- An output root is resolved (`..` components and symlinks) before it is refused for being inside
  the WIT root.
- `lock` refuses a released directory by its resolved path, so `lock .` and symlinks cannot
  bypass the refusal.
- `wit/released` accepts only plain directory names (`^[A-Za-z0-9][A-Za-z0-9._-]*$`).
- The release-freeze check reads the manifest's history, so a change that landed in a push whose
  CI run was cancelled is still caught, and a removed entry or deleted manifest fails.
- The MIT license names the witforge contributors as the copyright holder.

### Documentation

- Implemented lockfile v2 as specified and aligned the model/lockfile documentation with the
  shipped behavior.
- Adopted `aplan` for task-level planning, dependencies, and closure evidence; added the
  `aplan/` plan, `AGENTS.md`, and the `CLAUDE.md` symlink, with dogfooding notes in
  `docs/aplan-pilot.md`.
- Specified the internal model that isolates `wit-parser` from the lockfile and emitters.
- Specified the v2 lockfile format and its `check`/`lock` semantics, including typed entries,
  canonical types, function/parameter locking, draft/release handling, and version directories.
- Specified the Rust and protobuf emitter conventions, generated output layout, and `--check`
  behavior.
- Specified the protobuf glue API: free-function type conversions, per-function request/response
  helpers, the shared `GlueError`, and the `--plain-root`/`--proto-root` output layout.
- Specified proto3 `optional` presence for required scalar/string/bool/float/enum/bytes fields,
  parameters, and response results; the glue rejects absence for required values.
- Clarified required-field presence in proto3-JSON and the encode direction, and the structural
  projection of named list/option/result types.
- Emitter, lockfile, overview, roadmap, and release-checklist documentation now reflect the
  implemented 0.1 surface.
