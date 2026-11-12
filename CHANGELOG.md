# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Fixed

- Corrected the Component Model guide: wit-bindgen type remapping works for compatible records
  and variants, but not witforge enums, flags, or aggregates containing them with wit-bindgen
  0.63.0.
- Published the semantic decode rules non-Rust protobuf implementations must enforce, and
  documented the distinct plain-Rust Serde, WIT-shaped JSON Schema, and proto3-JSON projections.
- Documented that `diff` does not yet identify a new-to-old converter that must fail for every
  value.
- Removed the extra blank line at the end of generated `convert.rs`, and added a whole-tree CI
  whitespace check so committed output cannot preserve the same defect.
- Kept wide documentation tables inside the mobile viewport with local horizontal scrolling.

## [0.4.4] - 2026-11-12

0.4.4 publishes the generated-output, documentation, and CI hardening completed after 0.4.3. It
does not change the public library API, wire numbers, protobuf output, or JSON Schema output for a
valid contract accepted by 0.4.3. Generated Rust glue changes to avoid redundant clone and decode
work, and generated version adapters gain `# Errors` documentation; consumers that commit those
Rust outputs should regenerate when upgrading.

Release commit `07472e5` (tag `v0.4.4`), exact-commit main CI run 43143822291, release run
43144304257. Both crates are published; `cargo install witforge-cli --version 0.4.4 --locked`
reports `witforge 0.4.4`, docs.rs built the library, and the release carries four platform
archives with checksums. The tagged Action passed `check` and all four configured generator
checks on every release target.

### Changed

- The runnable examples now commit all generated Rust, protobuf, JSON Schema, and glue files.
  CI runs every emitter with `--check` against both examples, and the public guide shows exact
  generated-output excerpts with links to the full files.
- Generated Rust version adapters now carry `# Errors` documentation on fallible public
  conversion functions.

### Fixed

- Generated glue copies `Copy` primitives directly, clones borrowed owned values only when the
  protobuf target must own them, and moves owned identity lists and options during decode instead
  of rebuilding them through an iterator and a second allocation.
- CI installs the same published aplan 0.6.0 build that generated the repository's `AGENTS.md`
  block, eliminating the `APLAN041` plan-gate failure.

## [0.4.3] - 2026-11-11

0.4.3 closes dependency-validation and release-proof gaps found in the post-0.4.2 audit. It
does not change the library API, wire numbers, or generated output for a valid contract accepted
by 0.4.2. A contract that exposes an unsupported dependency type is now refused instead of
producing incomplete generated output.

Release commit `386cf1a` (tag `v0.4.3`), exact-commit main CI run 42964347193, release run
42964949155. Both crates are published; `cargo install witforge-cli --version 0.4.3 --locked`
reports `witforge 0.4.3`, docs.rs built the library, and the release carries four platform
archives with checksums. The tagged Action passed `check` and all four configured generator
checks on every release target; the pinned published Action then passed four-target CI run
42966331937.

### Fixed

- Dependency types reached transitively through root types, aliases, functions, and worlds are
  checked before generation. Unsupported dependency definitions remain skippable only when the
  root contract does not expose them.
- Dependency-version diagnostics distinguish member-documentation changes from wire-shape
  changes without allocating temporary order vectors.

### Changed

- CI executes the pinned published Action on Linux x86_64, macOS arm64, macOS Intel, and Windows
  x86_64. Tag releases execute the checked-out tag on the same four targets after uploading the
  matching archives.

## [0.4.2] - 2026-11-10

0.4.2 restores the published GitHub Action on GitHub's supported Node 24 runtime and hardens
dependency and release validation. It does not change the library API, wire numbers, or generated
output for a contract accepted by 0.4.1.

Release commit `1773519` (tag `v0.4.2`), exact-commit main CI run 42957300924, release run
42957872794. Both crates are published; `cargo install witforge-cli --version 0.4.2 --locked`
reports `witforge 0.4.2`, docs.rs built the library, and the release carries four platform
archives with checksums. The published Action passed `check` and all four configured generator
checks on a hosted runner.

### Fixed

- The JavaScript Action now declares Node 24, and CI executes the checked-out Action through
  `uses: ./` so a stale runtime declaration cannot pass unit tests unnoticed.
- A dependency interface imported or exported by a root world is validated as a complete API;
  unsupported required items are refused instead of silently omitted.
- Glue refuses dependency package names that normalize to one Rust module, and JSON Schema uses
  the same contract-error classification as the other generators.

### Changed

- Every generator and `diff` now use one detailed comparison requiring an identical pinned
  dependency graph across versions in a WIT root.
- The 1.0 and tag release gates deny public API breaks since 0.4.1, compare protobuf with
  historical release generators, require `ci ok` for the exact tag commit, and deny yanked
  dependencies.

## [0.4.1] - 2026-11-06

0.4.1 is a patch for dependency glue edge cases found after 0.4.0. It does not
change the library API, wire numbers, or generated output for a contract that
does not use dependency types.

Release commit `18fce07` (tag `v0.4.1`), main CI run 42300530773, release run 42301227582.
Both crates are published; `cargo install witforge-cli --version 0.4.1 --locked` reports
`witforge 0.4.1`, and the release carries the usual four platform archives.

### Fixed

- Nested dependency lists and options now use the same synthesized wrapper
  names as the protobuf emitter and prost, so `list<foreign list>`,
  `list<foreign option>`, and `option<foreign option>` round-trip.
- Foreign wrapper names are PascalCase without underscores, avoiding a
  mismatch between the `.proto` message name and prost's Rust name.
- The 1.0 conformance matrix covers the dependency, foreign-world, dependency
  conversion, and Serde fixtures.

## [0.4.0] - 2026-11-06

0.4.0 is the dependency and Serde release. It adds standard `deps/` dependency types,
generated dependency bindings, foreign world interfaces, dependency glue, and opt-in per-project
Serde derives. Generated output is unchanged for a contract 0.3.0 accepts unless the contract
uses `deps/` or the project enables `serde = true`.

Release commit `b61b75d` (tag `v0.4.0`), main CI run 42292759706, release run 42293555045.
Both crates are published through GitHub trusted publishing; `cargo install witforge-cli
--version 0.4.0 --locked` reports `witforge 0.4.0`, and the release carries Linux, macOS x86_64,
macOS aarch64, and Windows x86_64 archives with checksums.

### Added

- Standard `wit/deps` package dependencies with package-qualified type and interface
  references. Dependency Rust modules are emitted in `witforge_deps.rs`, protobuf schemas in one
  sanitized `<namespace>.<package>.v<major>.proto` file per dependency, and only referenced
  schemas are imported (`W9-001`, `W9-004`).
- Dependency aliases, lists, options, results, enums, variants, flags, records, and
  cross-dependency references are generated through their underlying wire shapes; a
  dependency's unsupported unused items are skipped instead of breaking the root (`W9-004`).
- Worlds may import and export interfaces from standard `deps/` packages. World metadata keeps
  the package-qualified interface name, protobuf imports the dependency schema, and glue
  converts foreign values and dependency interface functions through a shared
  `witforge_deps.rs` module (`W9-005`).
- Opt-in Serde derives for generated records, enums, variants, and flags, selected by
  `[project.<name>.rust] serde = true`, `--serde`, or overridden with `--no-serde`. The default
  output is unchanged (`W9-002`).

### Changed

- Glue for a contract with dependencies uses package-relative protobuf module paths
  (`<proto-root>::<namespace>::<package>::v<major>`) so prost's cross-package references
  resolve. Version adapters copy values whose types are the same pinned dependency type.
- The JSON Schema test dependency moved to `jsonschema` 0.60, and the CLI configuration
  dependency moved to `toml` 1.1; both are internal dependency updates with no runtime API
  change.

## [0.3.0] - 2026-11-06

0.3.0 is the change-review and CI release. It adds `witforge diff`, structured JSON/SARIF
diagnostics, a GitHub Action with prebuilt Linux/macOS/Windows binaries, and the quality and
security gates needed before 1.0. Generated output is unchanged for every contract 0.2.0
accepts; the new CLI options and release artifacts are additive.

Release commit `9ee341a` (tag `v0.3.0`), main CI run 42135517669, release run
42135977082. crates.io records both crates as published through GitHub trusted publishing;
`cargo install witforge-cli --version 0.3.0 --locked` reports `witforge 0.3.0`, docs.rs built
the library, and the release carries the Linux, macOS x86_64, macOS aarch64, and Windows x86_64
archives with checksums. The published Action was exercised against the tag for `check`, `diff`,
and a configured `rust --check` run.

### Added

- `witforge diff <old> <new>` compares version directories or WIT roots and reports added,
  removed, reordered, retyped, kind-changed, renumbered, and defaulted changes with independent
  wire-identity, source, and generated-API verdicts. `--json` emits the full report and
  `--check` exits non-zero on a breaking change (`W8-001`).
- `--format json|sarif` on `check`, `lock`, `diff`, and every generator emits a versioned
  diagnostic envelope or SARIF 2.1.0. Diagnostics carry stable codes, severities, paths, and
  optional source positions; human output is unchanged by default (`W8-002`).
- The repository publishes a GitHub Action and checksum-verified prebuilt archives for Linux
  x86_64, macOS Intel and Apple silicon, and Windows x86_64. The Action installs the matching
  release and can run `check`, `diff`, and configured generator checks (`W8-003`).
- Property/fuzz tests for arbitrary lockfile text and emitter input models, a small/large
  contract benchmark budget, `cargo audit` and `cargo deny` policy checks, and a public
  [`SECURITY.md`](SECURITY.md) reporting policy (`W8-004`).

### Changed

- CLI diagnostic collection and JSON/SARIF rendering are isolated from command dispatch; diff
  member comparison uses a structured comparison specification. Human output and exit codes are
  unchanged (`W8-006`).

## [0.2.0] - 2026-11-04

0.2.0 is the first pre-1.0 release of the full compiler surface. It adds generated
version conversions, declaration-order fingerprints, JSON Schema, world metadata,
`witforge.toml` projects, and the CI plan gate. Generated output changes for contracts with
multiple versions or worlds, and the schema generator adds a new output; consumers should
regenerate and commit their generated trees when moving from 0.1.2.

Release commit `dfb06de` (tag `v0.2.0`), CI run 41936185007, release run 41936720164;
crates.io records both crates as published through GitHub trusted publishing. The published
crates install with `cargo install witforge-cli --version 0.2.0 --locked`, and docs.rs built
the library.

### Added

- `witforge rust` now also emits `convert.rs` for a WIT root with two or more versions. It has
  one module per adjacent version pair in each direction, with fallible type conversions:
  old-to-new defaults `option<T>` and `list<T>` additions, new-to-old reports a removed required
  member as `ConversionError::MissingMember`, added or removed variant and enum cases report
  `UnknownCase`, flags default to `false`, aliases and named structural types convert
  recursively, and retyped or kind-changed types are refused as contract errors. A required,
  non-defaultable addition is refused so every accepted version pair has a generated adapter
  (`W6-007`).
- The lockfile gains an additive declaration-order section with per-parent FNV-1a 64
  fingerprints. `check --against` and `lock --against` refuse a reorder of members common to
  two versions, while additions and removals are ignored; a v2 lockfile without the section is
  accepted and migrates on the next successful `lock` (`W6-001`).
- `witforge schema` emits one Draft 7 JSON Schema document per contract version. Every named
  type is under `definitions`; records, flags, variants, results, enums, aliases, and nested
  `list`/`option`/`result` shapes are mapped, with all record fields required and closed enums
  (`W6-008`). The emitter writes JSON directly and adds no runtime dependency.
- `witforge.toml` projects can supply a WIT root and output paths for `rust`, `proto`, `schema`,
  and `glue`. `--config` selects the file and `--project` selects a project when there is more
  than one; CLI flags override every configured path, paths are relative to the configuration
  file, and unknown keys are errors. The existing command-line-only form remains valid
  (`W6-005`).
- Measured owned versus borrowed Rust parameters on generated alint types. Concrete clones cost
  0.58–0.69 ms for 1,000 analyzer rows and 7.1–9.0 ms for 10,000 rows, but borrowing would require
  a lifetime-bearing generated surface and does not remove native-to-WIT mapping. Owned parameters
  remain the 0.x/1.0 policy; borrowed inputs require a measured consumer path and a new contract
  version (`W6-002`).
- Evaluated conditional `Eq`/`Hash`/`Default` derives. They are declined for 0.x/1.0 because
  derives would change frozen generated APIs, floats block Eq/Hash transitively, and WIT has no
  natural Default for enums, variants, or results; a consumer request plus a new contract version
  would trigger an implementation task (`W6-003`).
- `rust` emits an additive shared `worlds.rs` when a contract has worlds. Each world exposes its
  name plus explicit imported and exported interface lists, with one `WORLDS` constant per
  version; per-version Rust modules remain frozen (`W6-004`).
- Published the 1.0 public API policy, conformance matrix, and release criteria/deprecation
  runways. The conformance suite exercises every supported WIT shape and all four emitters on
  the comprehensive model fixture (`W6-009`).
- Evaluated docs-only patch versions and a memberless-definition inventory. Both are declined for
  0.x/1.0: draft docs changes are accepted without wire changes, memberless additions have no
  wire entry, and released docs are caught by the release freeze (`W6-006`).
- Added `docs/quickstart.md` and runnable `examples/single` and `examples/evolved` trees covering
  all emitters, configuration, lock continuity, release freezing, and generated conversions
  (`W7-002`).

## [0.1.2] - 2026-11-01

The fixes from the fourth review (W6-013). Generated output is unchanged for every contract
0.1.1 accepts; the changes are validation, refusal, and `--check` behavior.

Release commit `3f89ec4`, CI run 41341872998, release run 41342094380; crates.io records both
crates as published through GitHub trusted publishing.

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
