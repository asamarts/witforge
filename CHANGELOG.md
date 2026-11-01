# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[Semantic Versioning](https://semver.org/).

## [Unreleased]

The fixes from the fourth review (W6-013).

### Added

- `witforge::ensure_directory_version`, which refuses a directory named `v<major>.<minor>` whose
  package declares another version; `check` and `lock` use it.

### Changed

- `check` requires the canonical lockfile only for a version that is not released, and reads
  CRLF line endings as LF for every version. A released lockfile, which `lock` refuses to
  rewrite, could never pass `check` again once a `core.autocrlf=true` checkout (the Git for
  Windows default) gave it CRLF, or when it carried a comment, as 0.1.0 accepted; the freeze
  check guards it instead. An unreleased lockfile in another form still exits `1` with the
  `witforge lock` command that rewrites it.

### Fixed

- `lock --lock` refuses a lockfile in a subdirectory of a released directory of another WIT root
  (`--lock other/v0.1/sub/wire.lock` with `other/released` listing `v0.1`): the manifest beside
  each ancestor of the lockfile's directory that is named like a version directory applies, at
  its written and its resolved location. The manifest was looked up only beside `sub/`.
- `check` and `lock` refuse a version directory whose package declares another version
  (`wit/v0.1` holding `@0.2.0`, exit `1`) or whose name spells a version with a sign or leading
  zeros (exit `2`), as the generators did.
- `glue` refuses every contract `proto` refuses, such as a record `f-request` beside a function
  `f`, whose request message is also `IFRequest`; it generated glue for a schema `proto` could
  not produce.
- The missing-lockfile hint is quoted for a POSIX shell and keeps `--lock` and `--released`, and
  every hint spells a path that starts with `-` with a leading `./` (`./-w/v0.2`), which clap
  would otherwise read as an option.
- `ci/check-release-freeze.sh` follows only renames of the manifest. `git log --follow` also
  follows copies, so a second WIT root whose manifest resembled the first's inherited the first
  root's history and failed, permanently. The check also gives git every path literally and reads
  paths back unquoted: a WIT root whose path held `*`, `?`, `[`, or non-ASCII characters failed
  with "no wire.lock tracked by git".
- The release workflow's `publish` and `github-release` jobs require a tag push. A manual run
  from a tag has a tag ref too, so it published, although the workflow and `ci/README.md` said a
  manual run publishes nothing.

### Documentation

- `docs/design/lockfile.md` says what `check` requires of a lockfile, that `lock` drops comments
  when it rewrites one, and that a consumer pins `wire.lock` and `*.wit` to LF in
  `.gitattributes`; and that a command run inside a symlinked version directory
  (`cd wit/v0.3 && witforge lock .`) sees only the link's target. The README no longer says every
  command exits `1` for a non-canonical lockfile; only `check` does.
- Tests now guard earlier fixes that no test failed without: internal emitter errors reported
  through each generator's plan as internal (exit `2`), the generators' package check for a
  contract with no members, their refusal of a symlinked version directory that no manifest
  lists, and the freeze check's exact-spelling test for `wire.lock`.

## [0.1.1] - 2026-11-01

The fixes from the post-publish reviews (W6-010, W6-011), and the first release published by
`release.yml` through crates.io trusted publishing.

Generated output is unchanged for every contract 0.1.0 accepts; the changes are refusals and
`--check` reports. Release commit `978c985`, CI run 41274574941, release run 41274576724;
crates.io records both crates as published through GitHub trusted publishing.

### Added

- `witforge::ensure_lockfile_not_released`, which `lock` uses to refuse a `--lock` path inside a
  released directory; `witforge::ensure_version_directory`, which refuses a symlinked version
  directory; and `witforge::is_released`.
- `WireLock::check_package`, which refuses entries of another package even for a contract with
  no members, and `WireLock::is_canonical`.
- A CI job that runs pinned actionlint and shellcheck over the workflows, with shellcheck's
  masked-return check enabled for every `run:` script, and shellcheck over `ci/`.
- `ci/check-release.sh` requires `witforge-cli`'s dependency on `witforge` to be the release
  version, since the CLI uses the library's API as of that version.

### Fixed

- Generation never replaces a symlink, a directory, or a file without witforge's header at an
  expected path: such a path fails generation with exit `2` before anything is written or
  removed. `--check` reports every such path as drift (exit `1`) instead of exiting `2` for a
  directory, blocking on a FIFO, or calling a hand-written file changed output, and it reads
  only regular files.
- No command follows a version directory that is a symlink: `check`, `lock`, the generators, the
  released manifest, and `ci/check-release-freeze.sh` refuse one, so a released version cannot
  change through a link's target. The generators used to skip one silently, and `lock` accepted
  the link's target directory, which no manifest listed.
- `lock` refuses a `--lock` path inside a released directory whatever `--released` names: the
  manifest beside the version directory and the one beside the lockfile's directory, each at its
  written and its resolved location, always apply, so neither a lockfile in another WIT root nor
  an explicit manifest that omits the directory gets past the manifest that lists it.
- The generators refuse a version directory name with a sign or leading zeros (`v+0.1`,
  `v00.02`) instead of reading it as a version.
- Lockfile keys from another package are refused in every mode, not only with `--against`, also
  for a contract with no members (by `check`, `lock`, and the generators), and a parameter key's
  interface must be a WIT identifier.
- `check` requires the lockfile exactly as `lock` writes it: comments, blank lines, tabs, CR line
  endings, a missing final newline, or another order exit `1` with "run `witforge lock`".
- The subset refuses another package in a version directory, from a nested `package` block or
  `deps/`. A nested package used to pass every check while nothing in it was locked or
  generated.
- The emitters mark a contract violation (a name collision, a WIT name with no Rust or proto
  spelling) where they find it, so an internal error exits `2` instead of being reported as a
  contract violation with exit `1`.
- `lock` leaves a byte-identical lockfile untouched instead of rewriting it.
- `check` names the exact `witforge lock` command, with its mode and paths quoted for a POSIX
  shell, in its hints; for a released version it says to restore the released WIT and lockfile
  instead of naming a command that `lock` refuses.
- The release workflow fails when the release check or the crates.io lookup fails. Both ran
  inside `echo "...$(...)"`, which hid the failure, so a tag that did not match the version, or
  a crates.io error, did not stop the run.
- `ci/check-release-freeze.sh` reads the manifest's full history, merges included, and checks a
  released directory against every commit that listed it, so a merge can no longer hide the
  commit that released it. It resolves the WIT root from the current directory (spellings such
  as `./wit`, `wit/../wit`, or a path from a subdirectory were silently left unchecked), fails
  outside a git repository or for a missing root, and refuses untracked files (ignored ones
  included) in a released directory. It refuses a released directory that is or holds a symlink,
  whose content git would compare only by the link's target, requires its `wire.lock` to be
  tracked under exactly the entry's spelling (on a case-insensitive file system `V0.1` matched
  `v0.1` and checked nothing), and follows the manifest across a move of the WIT root, so the
  move commit cannot change a release.
- `ci/test-publish.sh` reads the workspace version and tests with a version above any release,
  so the CLI is verified against the scratch copy of the library, not the published release.

### Documentation

- In the proto schema the wire numbers and reserved names are append-only, not the schema: a type
  or function removed as a whole drops its messages. The overview's evolution rules for a new
  minor are authoring policy that 0.1 does not enforce. Each generator needs its own output root.
  `ci/check-generated.sh` covers witforge's own fixtures; a consumer runs `--check` on its
  committed output.

## [0.1.0] - 2026-10-31

Published to crates.io from commit `546cec1` (tag `v0.1.0`), on which CI run 41255894743 passed.

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
