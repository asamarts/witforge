# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.1.0] - 2026-10-29

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
- `witforge rust`, `witforge proto`, and `witforge glue` commands, with highest-minor selection,
  deterministic output, ownership headers, stale-file cleanup, and `--check`.
- The representative alint plugin contract end-to-end proof, including compiled plain/glue output
  and binary/proto3-JSON round-trips.
- CI gates for generated output, released-directory freezes, append-only release manifests, and
  optional `buf breaking`.
- The 0.1 performance/ergonomics baseline and deferred-option triage.

### Changed

- Workspace and crate metadata now identify the 0.1.0 release candidate.

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
