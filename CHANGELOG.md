# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[Semantic Versioning](https://semver.org/).

## [Unreleased]

All library API changes since 0.7.0 are strictly additive (`docs/design/public-api-inventory.txt`
retains every v0.7.0 symbol and signature). Wire numbers and lockfile identity do not change.

Generated output changes:

- **JSON Schema (`W10-108`):** Doc comments are normalized consistently with Rust and protobuf
  (stripping `/** *` block-comment gutters), and member-level `description` fields are now emitted
  for documented record fields, flags, and variant cases (as well as record/variant/enum/flags type
  descriptions). Regenerate and commit JSON Schema files with `witforge schema` (or
  `witforge generate`).
- **Protobuf (`W10-107`):** Generated anonymous `Witforge*` wrapper messages now preserve exact
  spacing and deterministic doc comments, and unused dependency `import` statements (such as a
  world-only dependency interface not referenced by any proto message) are omitted. Regenerate and
  commit `.proto` files if affected.
- **Plain Rust & Glue (`W10-107`):** Deeply nested generic type signatures gain
  `#[allow(clippy::type_complexity)]` where required for `-D warnings`, and glue output matches
  `rustfmt` for long identifiers and `result<_, _>` expressions. Regenerate if affected.

### Added

- **Lifecycle CLI (`W10-109`):**
  - Added `witforge init [DIR] [--package <ns:name>] [--serde]` to scaffold a new contract
    workspace (`wit/v0.1/contract.wit`, `wit/v0.1/wire.lock`, and `witforge.toml`) with atomic
    lockfile creation and validation.
  - Added `witforge retire <WIT_DIR>` (with `--released` and `witforge.toml` resolution) to mark
    the oldest live released version `retired` in the released manifest while enforcing contiguous
    prefix and non-empty live-set rules.
  - Added `--from <VERSION>` to `witforge new-version` so a new minor or major can branch from an
    explicit base version rather than only the highest version.
- **Versioned contract IR export (`W10-110`):**
  - Added `witforge::inspect` (`IR_VERSION = 1`, `emit_version_json`, `inspect_version_json`,
    `inspect_root_json`, `summarize_version`, `summarize_root`) with zero new runtime dependencies,
    plus `WireLock::order_fingerprints`.
  - Added `witforge inspect [<PATH>] [--json] [--released]` (with `witforge.toml` discovery) to
    emit deterministic, machine-readable JSON contract IR (versions, packages, interfaces, types,
    functions, worlds, dependencies, stability metadata, and lockfile entries/order fingerprints)
    or a human summary for external tooling and custom code generators.
- **Opt-in `no_std` + `alloc` Rust, conversion, and glue codegen (`W10-111`):**
  - Added `witforge::emit::rust::{RustOptions, plan_with_options, emit_version_with_options}` and
    `witforge::emit::glue::{GlueOptions, plan_with_options, emit_root_with_options,
    emit_version_with_options}` (`#[non_exhaustive]`).
  - Added `--no-std` / `--std` CLI overrides on `rust`, `glue`, and `generate`, and `no-std = true`
    under `[project.<name>.rust]` and `[project.<name>.glue]` in `witforge.toml`. When enabled,
    generated plain Rust, `convert.rs`, `worlds.rs`, `witforge_deps.rs`, and glue reference
    `::core::` and `::alloc::` exclusively with zero `::std::` references.
- **Optional `witforge-build` helper crate (`W10-112`):**
  - Added the standalone workspace crate `witforge-build` (`Config`, `compile_protos`,
    `discover_protos`, `BuildOutput`, `Error`, `DEFAULT_INCLUDE_FILE`) to replace hand-written
    `build.rs` `protox` + `prost-build` boilerplate with deterministic `.proto` discovery, per-file
    `cargo:rerun-if-changed` emission, symlink refusal, automatic standalone vs. `deps/`
    `prost_include.rs` module layout matching `witforge glue`, and an optional `pbjson` feature for
    proto3-JSON `serde` implementations.
- **JSON Schema Draft 2020-12 and member descriptions (`W10-108`):**
  - Added `witforge::emit::json_schema::{JsonSchemaDraft, SchemaOptions, plan_with_options,
    emit_version_with_options}` (`Draft7` default, opt-in `Draft202012` emitting `$defs`, native
    `deprecated: true`, and Draft 2020-12 `$schema`).
  - Added `--draft 7|2020-12` on `witforge schema` and `draft = "7" | "2020-12"` under
    `[project.<name>.schema]` in `witforge.toml`.
- **Static `musl` Linux release archives (`W10-113`):**
  - Added `x86_64-unknown-linux-musl` and `aarch64-unknown-linux-musl` static release archives
    (with `.sha256` checksums and SLSA provenance attestations) alongside the existing `gnu`
    archives in `.github/workflows/release.yml`, and updated the GitHub Action to select static
    `musl` archives on Linux for releases `>= 0.8.0` (keeping `gnu` for `<= 0.7.x`) so older LTS
    distros and Alpine/musl containers work without a glibc 2.39 floor.
- **GitHub Action shorthand inputs and default token (`W10-102`):**
  - Added `check: "true"` (auto-runs `witforge check` using the discovered/configured project) and
    `generators: "all"` (expands to `rust,proto,schema,glue`) to `action.yml` while preserving the
    existing `generators: "rust"` default.
- **Additive library APIs (`W10-103`, `W10-105`, `W10-106`):**
  - Added `witforge::freeze::check_root_with_manifest` (custom released manifest path,
    semver-safe alongside `check_root`), `witforge::workflow::check_release`, and
    `witforge::diff::ChangeElement::ReservedEntry`.

### Fixed

- **GitHub Action default `github-token` (`W10-102`):** Defaulted `github-token` in `action.yml`
  to `${{ github.token }}` so `gh attestation verify` and `version: latest` API lookups no longer
  hit anonymous rate limits out of the box, while keeping public release-asset downloads
  unauthenticated; added a CI smoke step omitting `github-token`.
- **Release-freeze manifest overrides and retirement order (`W10-103`):** Added `--released` to
  `witforge freeze` and honored `project.<name>.released` from `witforge.toml`; enforced
  contiguous-prefix retirement-order and non-empty live-version checks directly inside `freeze` so
  invalid retirement edits fail freeze verification.
- **Compiler edge cases (`W10-104`):**
  - Enforced the canonical type nesting limit consistently during lockfile validation (`lock.rs`).
  - Fixed `SinceHistory` tracking across `@unstable` and absent intermediate versions so non-contiguous
    stability lifetimes are not conflated.
  - Refused malformed `@since`/`@deprecated` metadata consistently across root and dependency items.
  - Isolated `load_sources` in-memory source maps so spans never collide across calls.
  - Adjusted `tests/bench.rs` debug-profile ceilings to avoid spurious local debug-mode timeouts.
- **Diff wire-reuse and reserved-entry diagnostics (`W10-105`):**
  - Caught live wire-number reuse under `diff --check=decode` (not only under `wire`).
  - Reported deleted or modified same-major `reserved` lockfile entries via
    `ChangeElement::ReservedEntry` so reserved-number regressions cannot slip past `diff`.
- **Workflow, version-rewrite, and SARIF hardening (`W10-106`):**
  - Validated `witforge release` against the full WIT root in a single pass via
    `workflow::check_release` (catching cross-version lockfile or dependency mismatches before
    writing `released`), and removed duplicate per-version work inside `workflow::check_root`.
  - Fixed `witforge new-version` package-version rewriting to handle nested WIT block comments
    (`/* /* ... */ */`) without corrupting comments or missing the header.
  - Percent-encoded SARIF `artifactLocation.uri` paths (spaces, `#`, `?`, `%`, non-ASCII) and
    decoded `file:///` URIs cleanly in Action workflow annotations.
- **Emitter hygiene (`W10-107`):**
  - Prevented `clippy::type_complexity` failures on deeply nested Rust types and fixed dependency
    alias sizing / unit-error handling in plain Rust emission.
  - Fixed glue formatting edge cases so `rustfmt --edition 2021` and `2024` stay clean on long
    qualified paths and `result` arms.
  - Added proto3 `json_name` collision checking against reserved members, fixed wrapper message doc
    comments and blank-line spacing, and gated dependency `import` statements to actually referenced
    packages.
- **Documentation and comment drift (`W10-114`):** Reconciled post-0.7.0 drift across `AGENTS.md`,
  `crates/witforge/src/lib.rs`, `docs/design/{overview,lockfile,diff,diagnostics,dependencies,action}.md`,
  `.github/workflows/ci.yml`, `README.md`, and `docs/design/public-api-inventory.txt`.

## [0.7.0] - 2026-11-23

0.7.0 is the final planned pre-1.0 Rust-library source break. Library consumers must apply the
`W10-094`, `W10-095`, and `W10-096` migration notes below before moving their dependency.

Generated output changes for every contract that uses `@since`, `@unstable`, or `@deprecated`:
regenerate and commit plain Rust, protobuf, JSON Schema, glue, and shared metadata together.
Contracts without stability annotations do not gain those comments or deprecation attributes,
but should still run every generator with `--check`. Invalid annotation versions newly fail
validation as documented under `W10-094`. Wire numbers and lockfile identity do not change.

Release evidence: tag `v0.7.0` points to commit `34835c2`; exact-commit
[CI run 45257896510](https://github.com/asamarts/witforge/actions/runs/45257896510) passed, and
[release run 45265747346](https://github.com/asamarts/witforge/actions/runs/45265747346) published
both crates plus five checksum- and provenance-verified platform archives and passed the tagged
Action matrix. Alint [PR #647](https://github.com/asamarts/alint/pull/647) (`188defd5a`) regenerated
its consumer with the released binary and passed the consumer controls, 19 round-trip/inventory
tests, Clippy, and the full hosted CI suite. It is a pin-only regeneration of the unchanged draft
`0.1.0` contract, not the real contract-minor evolution still required for 1.0.

### Added

- **Adopter & contributor documentation and dual GitHub/Pages link checking (`W10-091`, `W10-092`):**
  - Added [`docs/guide/wiring.md`](docs/guide/wiring.md) (empty crate to compiling `prost`/`protox`
    round trip, verified by `wiring_guide_crate_generates_compiles_and_roundtrips`, prebuilt
    archive checksum/attestation verification, and `--lock`/`--released`/`released` options),
    [`docs/guide/reference.md`](docs/guide/reference.md) (complete CLI & `witforge.toml`
    reference verified against `Cli::command()`), and
    [`docs/guide/migration.md`](docs/guide/migration.md) (per-release generated-output migration
    index linking to `CHANGELOG.md` sections).
  - Added [`CONTRIBUTING.md`](CONTRIBUTING.md), GitHub issue forms (`.github/ISSUE_TEMPLATE/`),
    and `.github/PULL_REQUEST_TEMPLATE.md`; enabled `jekyll-relative-links` in `docs/_config.yml`,
    converted all internal `docs/` links to `.md`, and added dual GitHub/Pages link and anchor
    verification (`documentation_links_and_anchors_resolve_on_github_and_pages`).
- **Structure-aware fuzz target, tracked seed corpus, and asserted invariants (`W10-089`):**
  - Added a tracked fixture-derived seed corpus under `fuzz/seeds/`, a WIT keyword dictionary
    (`fuzz/wit.dict`), and the structure-aware `structured-contract` target exercising one-letter
    and upper-case names, multiple `.wit` files, and `deps/` packages.
  - Updated `wit-package` and `structured-contract` to assert full compiler invariants (generation
    success across all four emitters, determinism, canonical/idempotent lockfiles, empty
    self-diff, and two-version conversion) and added the matching fixed-seed property test
    `structured_contracts_satisfy_fuzz_invariants` to the stable suite.
- **GitHub Action ref-pinned versions, `against`/`format`/`sarif-file`/`binary`, annotations, and
  `failed-command` (`W10-086`):**
  - Non-tag refs (commit SHAs, branches, floating tags, and local `./`) now install the workspace
    version from `Cargo.toml` in the Action's own tree instead of silently floating to `latest`
    (`latest` is installed only when `version: latest` is passed explicitly; missing assets fail
    naming `version`).
  - Added `against`, `format` (`human`/`json`/`sarif`), `sarif-file`, and `binary` inputs plus
    `failed-command` and `sarif-file` outputs, pre-download input validation, bounded retries on
    transient network/5xx errors (never retrying 4xx or checksum mismatches), archive/checksum
    scratch cleanup, and GitHub workflow annotations (`::error file=...,line=...,col=...::...`)
    on failing checks.
- **Atomic releases, SLSA build provenance, Linux arm64, archive licenses, and crate metadata
  (`W10-087`, `W10-090`):**
  - Reordered `.github/workflows/release.yml` so platform binaries (including `linux/arm64` /
    `aarch64-unknown-linux-gnu`) are built, attested via `actions/attest-build-provenance`, and
    smoke-tested from workflow artifacts **before** publishing crates (using `--no-verify` after
    verifying `.crate` SHA-256 digests against the unprivileged `verify` job) or creating a
    draft-first GitHub release without `--clobber`. The Action verifies build provenance with
    `gh attestation verify` on releases `0.7.0+` before extracting.
  - Normalized Windows runner scratch paths before GNU tar extraction so the required Windows
    pre-publish archive smoke runs before trusted publication.
  - Corrected the immutable `crates-io-auth-action` v1.0.6 commit pin used for trusted OIDC
    publishing.
  - Included `LICENSE-APACHE`, `LICENSE-MIT`, and `README.md` in every release archive, added
    `homepage`/`documentation`/`[package.metadata.binstall]` crate metadata, excluded `tests/`
    from the published `witforge` library crate, enabled GitHub Private Vulnerability Reporting,
    documented the threat model and download verification in `SECURITY.md`, and ignored local-only
    paths (`/.private/`, `/.github-account`) in `.gitignore`.
- **CI concurrency, weekly schedule, pinned lint toolchain, and benchmark ceilings (`W10-088`):**
  - Grouped `main` CI runs per commit so no commit is cancelled while pending, pinned the gating
    Rust lint toolchain (`1.100.0`) with a weekly scheduled `floating-stable` + `fuzz-smoke` +
    `security` run, removed retired self-hosted runner leftovers and unused `ci/deny.toml`
    licenses, made every `ci/*.sh` script default scratch to `${TMPDIR:-$HOME/.tmp}`, cached
    historical `buf` baseline binaries in manual CI only, and tightened `tests/bench.rs` release
    ceilings to ~10x hosted-runner baselines.
- **Protobuf binary decode-compatibility view in `diff` (`W10-097`):**
  - Added a directional **decode** compatibility view (`new_reads_old` and `old_reads_new`) to
    `witforge diff`, modelling two processes speaking the same major's protobuf schema over binary
    encoding under the documented decode profile (distinguishing `option<T>` and non-byte `list<T>`
    from required fields and `optional bytes` `list<u8>`, closed enum/variant cases, flags,
    functions, and parameters).
  - Extended `diff --check=<views>` to accept `decode` (for example `--check=decode` or
    `--check=wire,decode`); bare `--check` continues to gate on `wire,source,api` without `decode`
    by default.
  - Added additive JSON fields (`decode_breaking`, `new_reads_old`, `old_reads_new` per change and
    `decode_breaking`, `new_reads_old_breaking`, `old_reads_new_breaking` in `summary`) and
    additive library types/functions (`diff::DecodeVerdict`, `diff::DecodeSummary`,
    `diff::FullDiff`, `diff::compare_full`).
- **Major-version boundaries, partial cross-major adapters, per-major dependencies, and version retirement (`W10-093`):**
  - Across a major-version boundary (`old.major != new.major`), `witforge rust` no longer refuses
    breaking changes: plain types for every live version are always written, `convert.rs` generates
    adapters for every type that still converts, and non-convertible types (and types containing
    them transitively) are omitted per direction with a deterministic sorted comment block at the
    top of `convert.rs`. Intra-major incompatible evolutions remain refused (`WF2002`).
  - `witforge diff` across a major boundary now permits changed, added, removed, or upgraded
    dependencies and reports the exact same per-direction omission set as `convert.rs` (human
    notes, `--json` `conversion_omissions`, `WF3004` notes in `--format json|sarif`, plus additive
    library APIs `diff::ConversionOmission`, `diff::compare_omissions`, and
    `diff::compare_with_omissions`).
  - Pinned dependency-graph identity is now enforced per major rather than across majors: identical
    dependencies stay at the shared `witforge_deps::<ns>_<pkg>` / `<ns>.<pkg>.v<dep_major>` paths,
    while dependencies that change or upgrade across live majors are scoped per root major across
    Rust, protobuf, JSON Schema, and glue.
  - Added `<name> retired` entries in `wit/released` (with `ReleasedManifest::is_retired`,
    `retired_directories`, `live_directories`, and `validate_retirement_order`): retiring a
    contiguous oldest prefix of versions keeps their directories frozen on disk while removing them
    from generated output (and drops a major's `.proto` and glue once all its versions are
    retired). `witforge freeze` accepts the one-way `live -> retired` transition and refuses
    un-retiring, removals, or edits to retired directories.
- **Built-in release-freeze verification (`witforge freeze`, `W10-083`):**
  - Added `witforge freeze [<WIT_ROOT>] [--config PATH] [--project NAME] [--base REV]` and the
    `witforge::freeze` library module enforcing the Git-history release-freeze rule across
    platforms (including shallow-clone refusal with exit `2` / `WF9004`, `core.autocrlf=true`,
    full-history rename tracking stopping at copies, symlink/untracked/case-sensitive `wire.lock`
    hygiene, multi-project config runs, and full violation collection under `WF1109`).
  - Reduced `ci/check-release-freeze.sh` to a thin wrapper over `witforge freeze` and added the
    `freeze` input to the GitHub Action.
- **High-level contract workflow commands and upward `witforge.toml` discovery (`W10-084`):**
  - Added `witforge generate`, `witforge new-version [--major]`, `witforge release <WIT_DIR>`,
    whole-WIT-root and config-driven `witforge check`, and upward `witforge.toml` discovery
    (stopping at `.git` or filesystem root, with `--no-config` opt-out and safe preservation of
    self-contained explicit CLI invocations).
- **Build identity, `--quiet`, per-view `diff --check`, shell completions, and `explain` (`W10-085`):**
  - `witforge --version` now includes the short Git commit (and `-dirty` when tracked files have
    local edits) on development builds, printing the bare version on exact clean release tags and
    builds without Git metadata (with no embedded timestamps).
  - Added global `--quiet` / `-q` to suppress human notes and summaries without suppressing errors,
    changing exit codes, or modifying JSON/SARIF output.
  - Extended `witforge diff --check` to accept `--check=<views>` (`wire`, `source`, `api`,
    comma-separated; defaults to all three) and mapped the GitHub Action's `diff-check` input onto
    it.
  - Added `witforge completions <bash|zsh|fish>` generated directly from the `clap` command tree,
    and `witforge explain <CODE>` backed by `witforge::diagnostic::explanation` as a single source
    of truth verified against `docs/design/diagnostics.md`.

### Changed

- **Consumer-visible stability and deprecation metadata (`W10-094`):**
  - Added `Stability::Deprecated` with the WIT introduction/feature origin, deprecation version,
    and deterministic note. Invalid annotation versions are refused, and WIT-root checks plus all
    generators verify `@since` against the first declaration when the claimed version is present.
  - Plain Rust now emits stability docs and `#[deprecated]`; protobuf emits stability comments,
    `deprecated = true` options, and deprecated method-enum values; JSON Schema emits stability
    descriptions and `"deprecated": true`. Generated Rust, prost, conversion, and glue code
    compile under denied deprecation warnings without suppressing warnings in consumer code.
  - `diff` reports introduction, unstable-feature, deprecation, and undeprecation changes as
    non-breaking `stability-changed` entries in human and JSON output.
  - **Library migration:** `ChangeKind` is deliberately exhaustive and gains the final pre-1.0
    `StabilityChanged { from, to }` variant; exhaustive 0.6 matches must add that arm for 0.7.
    Matches on `Stability` already require a wildcard because it is `#[non_exhaustive]`.

- **Extensible library data types and a machine-checked public API inventory (`W10-095`):**
  - Marked every public struct and enum in `model`, `lock`, and `diagnostic`
    `#[non_exhaustive]`, and added constructors for model values, lockfile inputs, source
    positions, and related locations. Downstream exhaustive matches now require a wildcard arm,
    and downstream struct literals must use the documented constructors, `Default`, parsers, or
    builders.
  - Added a pinned-rustdoc, machine-checked inventory of every public item, field and function
    signature, variant, associated item, explicit trait implementation, and extensibility
    attribute. Through the 0.7.0 tag commit, the gate requires the complete API delta from 0.6.0
    to equal the reviewed pre-1.0 inventory, while cargo-semver-checks separately requires its
    detected failures to equal the checked exception. On descendants of the tag, the tag's
    inventory becomes the signature-aware baseline: removals and type/signature changes fail,
    while reviewed additive entries remain possible. Removed the unused
    `emit::output::absolute` function and made the four internal model size-limit constants
    private.
  - **Library migration:** this is the planned pre-1.0 source break. Search library consumers for
    struct literals of `witforge::model`, `witforge::lock`, or `witforge::diagnostic` values and
    for exhaustive matches on their enums; replace literals with constructors and add wildcard
    match arms. Replace any use of `emit::output::absolute` with `std::path::absolute`, and do not
    depend on the former internal model limit constants.

- **Library-owned check/lock workflows, in-memory loading, and typed findings (`W10-096`):**
  - Added `SourceFile` / `load_sources` for deterministic in-memory root and textual `deps/`
    packages with physical source names and positions, and moved single-version/root `check` plus
    side-effect-free lock reconciliation into `witforge::workflow`. `LockReport` exposes canonical
    text and writes only through an explicit method; the CLI retains configuration parsing,
    rendering, and exit-status policy.
  - Replaced generator `Vec<String>` drift results with `GeneratedFileFinding` /
    `GeneratedFileStatus`, and replaced `diff::Change.element: String` with `ChangeElement`.
    Generated-output paths and diff categories are now typed end to end; the CLI no longer derives
    structure from message text. Human, JSON, and SARIF output remain unchanged.
  - **Library migration:** callers of emitter `check` functions inspect each finding's `path` and
    `status` (or call `message` / `diagnostic`) instead of parsing strings. Diff callers match
    `ChangeElement` instead of comparing `Change.element` with string literals. `witforge.toml`
    remains a CLI-only configuration model; library/build-script callers pass options directly.

- **Source positions, SARIF 2.1.0 physical/logical/related locations, and public diagnostic guide (`W10-080`):**
  - Structured diagnostics (JSON and SARIF) now report physical file paths (cwd-relative with `/`
    or percent-encoded `file:///` URIs outside cwd, never `..` or Windows `\\?\`), 1-based UTF-16
    line/column ranges (handling tabs, BMP/supplementary characters, CRLF, and leading UTF-8 BOMs),
    `logical` / `logicalLocations`, `related` / `relatedLocations` (pairing WIT declarations with
    their conflicting `wire.lock` line), SARIF rule `shortDescription` + `helpUri` pointing at
    the new public `docs/guide/diagnostics.md` reference, rendered `wit-parser` error snippets,
    and exit-aligned `diff` SARIF levels (`warning` on zero-exit runs, `error` on failing gates).

- **Per-rule diagnostic codes and cross-command machine-readable parity (`W10-079`):**
  - Subset violations previously reported under the single umbrella code `WF1001` are now split
    into one stable code per rule (`WF1010`–`WF1030`; `WF1001` remains for unrecognised constructs
    and is recorded in the **Prior** column of `docs/design/diagnostics.md` beside every split
    code). Consumers filtering JSON or SARIF reports on `WF1001` should update filters to the
    specific `WF1010`–`WF1030` codes or match the `WF10` prefix.
  - Generators (`rust`, `proto`, `schema`, `glue`), `diff`, `check`, and `lock` now emit the same
    diagnostic code, path, and message for every contract violation, and preserve multiple
    violations as separate diagnostics instead of joining them with `"; "` into a single `WF9000`.
  - `WF9000` is now strictly reserved for internal compiler errors. Usage/argument errors emit
    `WF9001` (including early argument-parsing errors when `--format json` or `--format sarif` is
    present on the command line, printed to stdout with exit `2`), filesystem/I/O errors emit
    `WF9002`, WIT/lockfile/manifest/config parse errors emit `WF9003`, and safety-guard/freeze
    refusals emit `WF9004`.
  - `lock --against` and `check --against` now both emit `WF1002` for a foreign-package lockfile,
    with `path` naming the offending file (the current lockfile or the baseline file).
  - `diff --json --format sarif` suppresses the diagnostic envelope on both success and error so
    `--json` always takes precedence.
- **Output mode preservation, unchanged-file skip, and `--check` / `--diff` explanations (`W10-076`):**
  - Generators now skip rewriting expected files whose on-disk bytes already match (preserving
    inode, modification time, and permissions), preserve replaced files' permission bits during
    atomic replacement, use process-unique temporary siblings (`.witforge.<name>.<pid>.<seq>.tmp`)
    so concurrent runs into one root do not delete each other's temporaries, and ignore unreadable
    unrelated files.
  - `--check` now names the regenerating command (preserving `--config` / `--project` or explicit
    paths) and prints the first differing line or a `core.autocrlf` CRLF-only hint for changed
    files. All four generator commands gain `--diff` to print a unified diff without changing exit
    status.
- **Stricter version-directory discovery, UTF-8 BOM handling, and config path guards (`W10-082`):**
  - Version-like entries starting with `v`/`V` followed by a digit that are not canonical
    `v<major>.<minor>` directories (`v0.2.0`, `V0.2`, `v0.2-rc1`, `v00.2`, `v1`, non-directories)
    are refused instead of silently skipped.
  - `wit/released` accepts a leading UTF-8 BOM; WIT parse errors note when a file begins with a
    UTF-8 BOM; `witforge.toml` refuses a leading UTF-8 BOM and refuses unexpanded shell paths
    (`~`, `$VAR`, `%VAR%`).
- **Accurate error remedies and runnable commands (`W10-081`):**
  - Reversed `diff` endpoints report the versions in the right order with a quoted runnable command;
    every missing/stale lockfile hint quotes paths and prints a runnable `--first` or `--against`
    command; `check` and `lock` on a WIT root list the discovered version directories with a
    runnable example command.
- **Specified `--serde` format-independence, `serde_json` limits, and JSON names (`W10-071`):**
  - Documented `--serde`'s format-independent derives, exact Rust-spelled JSON keys (`self_`,
    `type`, folded upper-case names, `"DoneNow"`, variant/flags/byte-list/result encodings),
    differences from JSON Schema reading rules and `convert.rs` cross-version defaults, and exact
    `serde_json` limitations (`Some(None)` on nested options, non-finite floats), pinned by an
    expanded `serde` fixture and compile/round-trip test.

## [0.6.0] - 2026-11-21

0.6.0 publishes the post-0.5.0 assurance work, the complete second-audit remediation for wire
identity, generated-code compilation, and tool safety, and the generated-output improvements for
clean Rust (`rustfmt`, default Clippy, `-D missing_docs`), hardened `convert.rs`, and complete
JSON Schema `result` shapes. The public `witforge` library API stays minor-compatible with 0.5.0
and MSRV remains Rust 1.88.

**Generated output changes for every existing consumer** (ownership headers, Rust formatting and
fallback doc comments, and, where applicable, upper-case Rust identifiers, doc-comment
normalization, JSON Schema `result`/`f32`, protobuf cross-package references/imports/wrappers,
`convert.rs` error fields, and multi-file ordering), and several previously accepted invalid or
ambiguous contracts and lockfiles are now refused. Read the migration section below before moving
a release pin and regenerating committed outputs.

Release commit `88fabf8` (tag `v0.6.0`), exact-commit main CI run 44946461491 (`ci ok`:
`https://github.com/asamarts/witforge/actions/runs/44946461491/job/136976143283`), release run
44957270999 (`https://github.com/asamarts/witforge/actions/runs/44957270999`). Both crates are
published; `cargo install witforge-cli --version 0.6.0 --locked` reports `witforge 0.6.0`,
docs.rs built the library, and the release carries four platform archives with checksums. The
tagged Action passed `check`, `diff`, and all four configured generator checks on every release
target.

### Migration

Every change in 0.6.0 that alters generated output or refuses a previously accepted contract or
configuration, and how to tell whether your contract is affected:

- **Contract-scoped ownership headers and distinct output roots (`rust`, `proto`, `schema`, `glue`):**
  Every generated file now opens with `// @generated by witforge (<gen>, <pkg>); do not edit.`
  (or `{"$comment":"@generated by witforge (schema, <pkg>); do not edit.",` for JSON Schema).
  *How to tell:* every existing consumer's `--check` fails on pre-0.6.0 output once until you
  regenerate (generation automatically adopts files with the legacy shared header), and any
  consumer script that asserts the exact first line (such as a hand-written ownership check) must
  match the new per-generator header. Any `witforge.toml` whose `out` paths coincide or nest, or
  whose output root resolves to an ancestor of or overlaps the WIT root or any released directory,
  now exits `2` before writing.
- **Generated Rust formatting, fallback doc comments, unit signatures, and type-only interfaces:**
  Generated Rust now matches default `rustfmt` (Editions 2021 and 2024), passes default Clippy,
  documents every public item (`-D missing_docs`), omits `-> ()` on unit-returning trait methods,
  and stops emitting an empty trait for type-only interfaces (zero functions). *How to tell:*
  every generated Rust file updates once on regeneration; compile your consumer crate after
  regenerating to find any reference to an empty trait for a type-only interface (such as
  `types::Types`) and remove it. If your crate uses non-default `rustfmt.toml` settings, keep
  `#[rustfmt::skip]` on the module including the generated tree.
- **WIT documentation normalization, escaping, and placement:** Package, world, and
  dependency-interface docs are now emitted, `.proto` interface docs move above
  `enum <Interface>Method`, control/line-separator characters are normalized, and code blocks,
  `<...>`, and non-external `[...]` links are escaped for rustdoc. *How to tell:* regenerate and
  inspect diffs on any contract whose `.wit` files carry package, world, interface, or
  Markdown-special doc comments.
- **Plain Rust identifiers for upper-case WIT words:** Upper-case words now fold to idiomatic
  Rust casing (`URL` -> `url`, `HTTP-api` -> `http_api` / `HttpApi`, `HTTP` -> `Http`,
  `interface API` -> `mod api` and `trait Api`), and `--serde` JSON keys follow those plain Rust
  spellings. `.proto` and JSON Schema names do not change. *How to tell:* search your `.wit`
  files for ASCII uppercase letters (`[A-Z]`) in interface, type, field, case, flag, parameter, or
  function names, and check for compiler errors or changed `--serde` keys after regenerating.
- **`ConversionError` now carries `from` and `to` fields:** In generated `convert.rs`,
  `ConversionError::MissingMember` and `ConversionError::UnknownCase` gain `from: &'static str`
  and `to: &'static str`. *How to tell:* search consumer crates that include `convert.rs` for
  `ConversionError::MissingMember` or `ConversionError::UnknownCase`; destructuring patterns
  without `..` fail to compile until `from`, `to`, or `..` is added.
- **JSON Schema `result` forms and `f32` bounds:** Every `result` form (`result<T>`,
  `result<_, E>`, bare `result`, `result<T, E>`) now emits a closed `oneOf` of `{"ok": ...}` and
  `{"err": ...}` with `{"type":"null"}` for payload-less sides, and `f32` adds finite min/max
  bounds. *How to tell:* search `.wit` files for `result<_,`, single-argument `result<T>` (no
  comma), bare `result`, or `f32`, or inspect the diff of regenerated `*.schema.json` files.
- **Byte-list aliases (`list<byte>` -> `bytes` / `Vec<u8>`):** Any `list<T>` whose resolved
  element type is `u8` (including local, `use ... as ...`, and dependency aliases) now projects
  to protobuf `bytes`, Rust `Vec<u8>`, and JSON Schema 8-bit integer arrays instead of `repeated
  uint32` / `Vec<Byte>`. *How to tell:* regenerate and check for `.proto` fields changing from
  `repeated uint32` to `bytes` or Rust fields changing from `Vec<Alias>` to `Vec<u8>`. Mixed
  deployments agree on values `< 128` and disagree on values `>= 128`.
- **Protobuf cross-package references, unused imports, and structural wrappers:** Cross-package
  `.proto` references gain a leading dot (`.namespace.name.v<major>.<Type>`), unused dependency
  imports are dropped, unreferenced named `list`/`option`/`result` types now emit their wrapper
  messages, and dependency structural wrappers are owned by the dependency package that writes the
  shape. *How to tell:* regenerate `.proto` and `glue` for any contract with `deps/` or named
  `list`/`option`/`result` definitions; binary descriptors are unchanged for existing fields.
- **Multi-file WIT deterministic declaration order:** Multi-file version directories and `deps/`
  packages now load `*.wit` files in ascending file-name byte order. *How to tell:* only
  contracts with more than one `.wit` file in a version directory or `deps/` package are affected;
  wire numbers and `wire.lock` entries are unchanged, and generated files may reorder declarations
  once on regeneration.
- **Glue requires absolute `--plain-root` and `--proto-root`:** Relative paths (`self::...`,
  `super::...`) and unprefixed generated module names (`v<major>`, `witforge_deps`, etc.) now
  exit `2`. *How to tell:* check `--plain-root` / `--proto-root` flags and `witforge.toml`
  `plain_root` / `proto_root` keys; rewrite relative paths to `crate::...` or `::crate_name::...`.
- **GitHub Action `diff-old`, `diff-new`, `diff-check`, and `github-token` inputs:** Releases
  through `v0.5.0` ignored these inputs. *How to tell:* grep workflow files for `diff-old:` or
  `diff-new:` on `asamarts/witforge@`; moving to `v0.6.0` starts executing
  `witforge diff ... --check` (which fails on source-breaking changes such as added record
  fields) unless you set `diff-check: "false"`.
- **Unreleased lockfiles require a complete `# witforge order v1` section:** Every mode (`check`,
  `check --first`, `check --against`, and all generators) refuses an unreleased `wire.lock`
  missing or omitting entries from `# witforge order v1` with exit `1`. Released lockfiles written
  before the section existed remain accepted and unmodified. *How to tell:* run
  `witforge check <version-dir>`; if an unreleased draft predates order fingerprints, run
  `witforge lock <version-dir>` once to append `# witforge order v1` without changing wire numbers.
- **Stricter `check`, `lock`, and generator refusals (including frozen released directories):**
  `witforge check` now refuses every generator-only collision (`WF1003`: type/trait, `functions` /
  `witforge_value` / `UNSPECIFIED` / `Empty`, `Request`/`Response`, ASCII case, prost identifier
  and accessor, dependency module/filename, and synthesized wrapper collisions), non-`<major>.<minor>.0`
  `deps/` versions, named interface imports/exports, unsupported annotations, type expansion
  beyond 32 levels / 32 alias links / 1024 bytes, `--first`/`--against` predecessor skips,
  symlinks inside listed `wit/released` directories (exit `2`), and three conditions that can make
  an already-released, frozen directory unusable because it cannot be edited in place:
  1. a lockfile parent mixing live and reserved member kinds (e.g. a released type that changed
     kind, `W10-045`);
  2. a dependency graph pinning two versions of one `namespace:name` or reusing the root package
     name (`W10-047`);
  3. a lockfile violating the documented grammar (`W10-053`, see `docs/design/lockfile.md`
     section 17).
  *How to tell:* run `witforge check` and your configured generators with `--check` against every
  version directory. No repository fixture, example, or `alint` contract hits these frozen-directory
  refusals; any affected contract recovers by cutting a new version with a fresh type name or
  single pinned dependency version.

### Added

- Added the `cargo-fuzz` harness (`fuzz/`) for WIT and lockfile inputs with CI, Dependabot, and
  security-gate integration, expanded property-test and conversion/dependency/diagnostic coverage,
  and added two-version diff and conversion benchmark budgets.
- `lock --against` restores a released member that the current draft removed when the source
  adds it back with the baseline's number, kind, and type. The entry is un-reserved rather than
  refused as a reintroduction; restoring a different kind or type, or a member already reserved
  in the baseline, is still refused. Without `--against` the reintroduction message points at
  the baseline option.
- `proto` reserves every old member of a type whose kind changed, not only entries of the
  current kind, using the name the old projection gave it. A removed interface keeps its
  method-id enum with `UNSPECIFIED` and its reserved method ids and names, because the per-major
  proto file is shared by every minor of the major.
- `diff` reports a non-defaulted required record field addition as a conversion-generation
  blocker in human, `--json`, JSON diagnostics (`WF3004`), and SARIF output. The note is
  informational; compatibility counts and exit codes are unchanged.
- CI checks generated-output documentation snippets and README/docs/Action current-version
  references.
- The test suite runs the command sequences of the README, the quickstart, and the evolution
  guide as written, and `cargo test` runs the release-script cases and the Action's unit tests.
- Added a shared `prost-build` 0.14 / `pbjson-build` 0.9 compile-and-round-trip test harness
  (`crates/witforge/tests/support/mod.rs`) and `compile_matrix` integration suite covering name
  matrices (one-letter, upper-case, digits, all Rust 2015–2024 keywords, and all proto3 keywords),
  shape matrices (every kind in every position, three-level nesting, both byte-list spellings,
  unreferenced structural types, and two-hop dependencies), a fixed-seed property sample, and the
  committed `examples/single` and `examples/evolved` outputs in both Edition 2021 and Edition 2024
  with `rustfmt`/`cargo fmt --check`, `cargo clippy -D warnings`, `-D missing_docs`,
  `cargo doc -D warnings`, `cargo test --doc`, and protobuf binary/JSON round-trips; also wired
  committed-example compilation into `ci/check-generated.sh` and CI.

### Changed

- `witforge check` (and `subset::check_diagnostics`) now reports every single-version refusal
  that `rust`, `proto`, `schema`, or `glue` could make—including type/trait collisions, reserved
  `functions` / `witforge_value` / `UNSPECIFIED` / `Empty` names, `Request`/`Response` collisions,
  dependency module/filename collisions, synthesized wrapper collisions and 1024-byte limits,
  model expansion limits (`WF1001`), and live-vs-reserved lockfile collisions (`WF1003`)—so a
  passing `check` implies all four generators succeed. Unreachable unsupported dependency items
  and every dependency item transitively referencing them (including across multiple dependency
  packages) are now pruned to a fixed point during model construction rather than failing with
  exit `2` or emitting dangling aliases/`$ref`s. Protobuf package segments now accept proto3
  keywords accepted by `protoc`/`protox` (such as `package example:service@0.1.0;`). **Migration:**
  if a released contract version directory was previously accepted by `check` while carrying a
  generator-only collision (such as `record get-request` beside `get: func()`, `record functions`,
  or colliding dependency modules), the extended `check` now reports it with `WF1003` (exit `1`).
- Generated Rust (plain Rust, `worlds.rs`, `convert.rs`, `witforge_deps.rs`, and protobuf glue)
  now formats directly in `rustfmt`'s default style for both Edition 2021 and Edition 2024
  (without invoking `rustfmt`), passes default Clippy lints (`-D warnings`), and documents every
  public item (`-D missing_docs`, emitting a concise one-line comment naming the WIT identity when
  no WIT doc comment is present). Type-only interfaces (zero functions) no longer emit an empty
  trait or root re-export—so `interface error { enum error { a } }` is now valid—while their empty
  `<Interface>Method` enum stays in `.proto` to preserve wire/schema history under `buf breaking`.
  Unit-returning trait methods omit `-> ()` (`fn ping(&self);`). **Migration:** consumers naming
  an empty trait for a type-only interface (such as `types::Types`) must remove that reference;
  regenerating released per-version Rust modules updates their formatting, fallback doc comments,
  and unit signatures once. If your crate sets non-default `rustfmt.toml` options, put
  `#[rustfmt::skip]` on the module declaration that includes the generated tree.
- Generated `convert.rs` adapters no longer emit untyped `Result::Ok(...)?` expressions or
  redundant closures, so adding consumer `From<...> for ConversionError` impls in the including
  crate compiles cleanly and generated code passes Clippy at the default level. Empty version
  pairs omit unused imports, reverse-conversion doc comments state that added fields and flags are
  dropped without reporting them, `ConversionError`'s doc comment explains the scope of
  `#[non_exhaustive]`, and both `ConversionError` (`from` and `to` `'static` fields) and
  conversion-generation refusals now name both versions and the direction. **Migration:** in
  crates that include `convert.rs`, patterns destructuring `ConversionError::MissingMember { path }`
  or `ConversionError::UnknownCase { path, case }` must add `from`, `to`, or `..`.
- Draft 7 JSON Schema now represents every `result` form (`result<T, E>`, `result<T>`,
  `result<_, E>`, and bare `result`) as a `oneOf` of closed single-key `{"ok": ...}` and
  `{"err": ...}` objects where a payload-less side has schema `{"type":"null"}` (matching
  payload-less variant cases), so every `Ok(())` and `Err(())` value has a unique valid document.
  `f32` schemas now include finite bounds (`"minimum":-3.4028235e38,"maximum":3.4028235e38`), and
  `docs/design/emitters.md` section 10.2 documents the `f32` precision limit, the 2^53 IEEE 754
  double caveat on 64-bit integers, Draft 7's `integer`/`1.0` rule, and `option<option<T>>`'s
  shared `null`. **Migration:** `result<T>`, `result<_, E>`, bare `result`, and `f32` schemas
  change on regeneration. To find affected contracts, search your `.wit` files for `result<_,`,
  single-argument `result<T>` (no comma), bare `result`, or `f32`, or inspect the diff of
  regenerated `*.schema.json` files (previous schemas emitted `{"maxProperties":0}` for bare
  `result` or a single-branch object without `oneOf` for one-sided results).
- WIT documentation comments (`///` and `/** ... */`) are now normalised and escaped for rustdoc
  and protobuf before emission: control characters and Unicode line separators (`\r`, NUL, VT, FF,
  `U+0085`, `U+2028`, `U+2029`) are normalised (and escaped in JSON Schema `description`),
  indented and fenced code blocks become closed `text` fences so they never run as Rust doctests,
  HTML-like `<...>` and non-external `[...]` links are escaped while real `[text](url)` links keep
  working, bare URLs are wrapped in `<...>` in Rust, and package, world, and dependency-interface
  comments are now carried to generated output. In `.proto` output, interface comments now attach
  directly above `enum <Interface>Method` instead of floating above the next declaration.
  **Migration:** generated files whose WIT sources carry package, world, dependency-interface,
  interface, or Markdown-special comments change once when regenerated.
- Plain Rust, protobuf, and glue now derive all identifiers from one shared conversion module
  that reproduces `prost-build` 0.14's default naming (case conversion, `strip_enum_prefix`,
  oneof module names, and method enum zero values) directly from the stable `.proto` names.
  `check` (`WF1003`) and `proto`/`glue` refuse pairs of WIT names that map to one generated
  identifier (such as `api-x/r` beside `api/xr` both becoming `ApiXr`). **Migration:** `.proto`
  message, field, and enum-value names (and JSON Schema names) do not change; plain Rust
  identifiers for upper-case WIT words now fold to idiomatic Rust casing without `non_snake_case`
  or `non_camel_case_types` warnings (for example `URL` -> `url`, `HTTP-api` -> `http_api` /
  `HttpApi`, `foo-BAR` -> `foo_bar` / `FooBar`, `HTTP` -> `Http`, `interface API` -> `mod api`
  and `trait Api`), and `--serde` JSON keys follow those plain Rust spellings when regenerated.
- Glue now matches `prost-build` 0.14's exact keyword escaping table for prost fields, flags,
  function parameters, and proto package module segments (including `extern` -> `extern_`,
  `union` -> `union`, Edition 2024 `gen` -> `r#gen`, and keyword dependency package segments such
  as `example:gen` and `self:super`), and `check` (`WF1003`), `proto`, and `glue` refuse records
  and functions whose fields collide on a `prost-derive` 0.14 generated accessor (such as
  `mode: my-enum` beside `set-mode: bool` sharing `set_mode`, or `modes: list<my-enum>` beside
  `push-modes: bool` sharing `push_modes`).
- Dependency structural aliases (`list<list<...>>`, `option<list<...>>`, `option<option<...>>`,
  named and inline `result`, transitive dependency aliases, and byte-list aliases including
  aliases through `u8`) now define each wrapper in the proto package whose WIT source writes the
  shape, reference dependency wrappers cross-package with the matching `import` and prost path in
  both root and dependency glue, and project foreign byte-list aliases to `bytes` / `Vec<u8>` in
  every field, list, option, result, variant, parameter, and function-result position.
- `glue` now runs the protobuf emitter's full validation for the root and every dependency so
  `glue` refuses every contract `proto` refuses (including collisions inside a dependency) with
  the same diagnostic and exit `1`. Unset option wrappers now return `GlueError::InvalidState`
  consistently on every decode path (local named, anonymous, and foreign named options),
  synthesized glue helpers now carry doc comments naming their WIT type or function, and empty
  doc lines in plain Rust, protobuf, and glue no longer emit trailing whitespace.
- `glue` now requires absolute module paths for `--plain-root` and `--proto-root` (and the
  matching `witforge.toml` keys): `crate`, `crate::...`, `::crate_name::...`, or an external
  crate name (`other::...`). Relative paths starting with `self` or `super`, and unprefixed
  roots starting with a generated module name (`v<major>`, `witforge_deps`, interface/type/
  function/dependency modules), are refused with exit `2`.
- Each generator now writes a distinct ownership header naming both the generator (`rust`,
  `proto`, `schema`, or `glue`) and the root contract (`namespace:name`), for example
  `// @generated by witforge (rust, example:plugin); do not edit.` and
  `{"$comment":"@generated by witforge (schema, example:plugin); do not edit.",`. An output root
  holding files from another generator or another contract is refused with exit `2` before
  anything is written (and `--check` reports the foreign owner rather than listing an extra file),
  and `witforge.toml` configurations whose `out` paths coincide or nest are refused as a usage
  error. **Migration:** every generated file changes its first line once, so `--check` fails on
  pre-0.6.0 output until you regenerate; generation automatically adopts and rewrites files
  carrying the legacy shared header.
- The lockfile reader now enforces the documented grammar: an entry's kind must match its key
  shape, canonical type tokens are parsed rather than checked for a slash, only ASCII spaces and
  tabs separate fields, order parents use the key grammar, and order fingerprints are exactly
  sixteen lowercase hexadecimal digits. These are parse errors (exit `2`) naming the line; the
  migration forms are listed in `docs/design/lockfile.md` section 17.
- `diff::compare` no longer collects conversion-impact notes it discards, and the new `diff`
  report types are documented as fixed for 1.0. `emit::proto::emit_major` now reports an
  inconsistent public model instead of panicking.
- Emitter naming and type-rendering helpers now share one implementation; generated output is
  unchanged.
- Generation replaces each generated file atomically: it writes a temporary file beside the
  target and renames it into place, and it removes stale generated files only after every
  expected file has been written. A failed or interrupted run no longer leaves a truncated
  generated file.
- Every command that reads `wit/released` refuses a symlink anywhere inside a listed version
  directory (a symlinked `contract.wit`, `wire.lock`, or `deps/` entry) with exit `2`. Releases
  through 0.5.0 refused only a listed directory that was itself a symlink.
- The GitHub Action aborts every GitHub API and release-asset request after 30 seconds, so a
  stalled network fails the step instead of waiting for the job timeout.
- The release gates compare the library API with the newest published release and the protobuf
  output of every fixture with every published release. Both gates previously named 0.4.1.
- The JSON Schema test dependency moved to `jsonschema` 0.61, and the locked version of the
  CLI's `toml` dependency moved to 1.2.1; both are internal dependency updates with no runtime
  API change.

### Fixed

- `proto` now writes cross-package protobuf type references with a leading dot
  (`.namespace.name.v<major>.<Type>`) so innermost-first resolution cannot shadow a dependency
  whose namespace matches a segment of the root package, and emits sorted `import` lines only
  for dependency files referenced by an emitted type (dropping unused imports for world-only or
  inlined-scalar dependencies). **Migration:** text-only change in dependency `.proto` files;
  compiled protobuf descriptors and `prost-build` output are unchanged.
- `proto` now synthesizes the wrapper messages needed by every named `list`, `option`, and
  `result` type whether or not any field, case, or function refers to it, keeping `proto` and
  `glue` in step for unreferenced named structural types.
- Multi-file version directories and multi-file `deps/` packages now load their `*.wit` files in
  ascending file-name byte order rather than raw OS directory-enumeration order, making generated
  output deterministic across file systems and platforms. **Migration:** single-file contracts,
  wire numbers, and `wire.lock` files are unaffected; an existing multi-file contract's generated
  output may reorder declarations once into file-name byte order when regenerated.
- The generator output-root guard now resolves existing path components canonically (following
  symlinks, dangling symlink targets, Windows junctions, and case-insensitive spellings), refuses
  `..` after a missing directory component (closing the `missing/../witlink/v0.1` bypass),
  refuses an output root that contains the WIT root, and applies the same guard to every directory
  listed in the `released` manifest for both generation and `--check`.
- `witforge lock` now writes `wire.lock` atomically before reporting its changes, and CLI stdout
  and stderr writes no longer panic when piped to a reader that exits early (`head`, `grep -q`)
  or redirected to a full device: a closed stdout pipe preserves the command's exit status, while
  a non-pipe stdout write error (such as `/dev/full`) exits `2` after any file write completes.
- Subset validation now reports every dependency package under `deps/` (referenced or not) whose
  version is missing or not `<major>.<minor>.0` as a subset diagnostic (`WF1001`, exit `1`) naming
  the package, version, and `deps/` entry instead of failing `model::build` with exit `2`. Named
  interface imports and exports (`import foo: types;`, `export bar: types;`) are refused with their
  own message rather than as inline interfaces, annotations other than `@since`, `@unstable`, and
  `@deprecated` (including `@external-id`) are refused, diagnostics for unsupported dependency
  items pulled in by a world name the world item and note implicit imports, and two names in one
  scope that differ only in ASCII letter case (including a live member colliding with a reserved
  lockfile entry) are refused by `check` and `lock` with code `WF1003` and exit `1`.
- A lockfile parent whose live and reserved members have different kinds is refused by `check`,
  `lock`, and every generator with exit `1`, naming the parent and both kinds. A released type
  that changes kind (enum to record, and every other ordered pair) previously kept the old
  cases reserved beside the new members while `check --against`, `proto`, `glue`, and `schema`
  accepted the contract. `diff` now reports the change as wire-breaking, because every field
  that refers to the type keeps its canonical name while its encoding changes. An unreleased
  draft that already mixes kinds recovers by renaming the type in a new version (or by
  recreating the draft with `lock --first`, which drops its draft entries); a released
  directory in that state cannot be generated again and needs a new type name.
- A list whose resolved element type is `u8` now projects as protobuf `bytes`, Rust
  `Vec<u8>`, and a JSON Schema array of 8-bit integers in every spelling: `list<u8>`,
  `list<byte>` with `type byte = u8`, an alias chain, a `use ... as ...` alias, or an alias
  declared in a dependency. Previously `list<byte>` was `repeated uint32` in protobuf and glue
  while `list<u8>` was `bytes`, although both lock as `list<u8>`; respelling changed the wire
  encoding under an unchanged lockfile. Regenerate proto, glue, plain Rust, and JSON Schema
  output. A consumer detects an affected contract by a field that changes from `repeated
  uint32` to `bytes` (or, in Rust, from `Vec<Byte>` to `Vec<u8>`); mixed deployments read the
  same bytes when every value is below 128 and different bytes above it, because both
  encodings are length-delimited.
- A dependency graph that contains two versions of one `namespace:name`, or a dependency that
  reuses the root package's `namespace:name`, is refused by `check`, `lock`, `diff`, and every
  generator with exit `1`, naming the package, versions, and `deps/` entries. Dependency type
  identity has no version, so two versions previously collapsed into one package in the lockfile
  and emitters. One graph must pin one version, and a new major is adopted by every dependent at
  once; a contract that needs both uses separate WIT roots. A released directory already in that
  state is frozen and becomes unusable, so it needs a new version that chooses one version (or
  renames the root package). No fixture, example, or known contract has such a graph. A
  same-version duplicate in two `deps/` entries is refused by the same pre-scan instead of
  surfacing as a `wit-parser` parse error.
- `diff` compares function method ids and reports a new member that takes a number reserved in
  the old lockfile as wire-breaking. Previously a renumbered interface printed `no contract
  changes … COMPATIBLE` while the protobuf emitter refused it, and a field or function that
  reused a reserved number was reported as a compatible addition.
- `lock --first` and `check --first` are refused with exit `1` when the WIT root already holds
  an earlier minor of the same major; the message names the predecessor and the `--against`
  command to use. A baseline that is the `wire.lock` of a sibling version directory must be the
  immediate predecessor in the same major: a baseline from another major, or one that skips an
  existing intermediate minor, is refused with the `--first` or predecessor remedy. A baseline
  outside a version directory is accepted as given.
- An unreleased lockfile must carry a complete `# witforge order v1` section: exactly one line
  for every parent with live members, naming exactly those members in declaration order. A
  missing section, a deleted line, a subset, a non-member, an unknown parent, or a wrong order
  is exit `1`; `check` applies the rule in default, `--first`, and `--against` modes, and every
  generator refuses a lockfile that fails it. Previously deleting the section let a reordered
  record or parameter list pass `check`, `rust`, and `proto`, and `check --first` skipped the
  comparison. A released lockfile written before the section existed has no marker and stays
  accepted and unmodified; once a released lockfile has the marker, a wrong section is refused
  rather than rewritten. Direct `diff` endpoints still accept stale order so they can report a
  reorder, but refuse an unknown parent or a line naming a non-member, subset, or wrong set.
- Synthesized wrapper messages are looked up by the type they wrap, not by their generated name
  alone. Two different types that would produce one wrapper name are now refused with exit `1`
  and a message naming both canonical types and the wrapper; previously the second silently
  reused the first wrapper's schema. Aliases and the byte-list spellings that resolve to one
  projection still share one wrapper. No existing wrapper name changes.
- Generated conversions expand aliases, `use … as …` aliases, named `list`/`option`/`result`
  definitions, and dependency aliases before comparing types. Changing `id` to `u32` (or the
  reverse), or a named list to its expansion, now generates the identity conversion instead of
  making `rust` refuse a pair that `check --against` and `diff --check` accepted. A changed
  alias target remains a real type change and is refused. `diff` now lists retyped record
  fields, variant cases, and types under `conversion_blockers`, so a passing `diff --check`
  implies `rust` generation succeeds.
- Type expansion is bounded: at most 32 nested `list`/`option`/`result` levels, 32 alias links,
  and 1024 bytes per canonical type string. Every definition and function in the root and its
  dependencies is checked after model construction, the lockfile reader applies the same nesting
  and byte limits to a token, and wrapper message names are capped at 1024 bytes. Over-limit
  contract types are contract errors (exit `1`); an over-limit lockfile token is a parse error
  (exit `2`). Number assignment for a wide contract is now linear in the member count instead of
  rescanning every entry for every new member.
- Canonical type strings now document the foreign form they already used: a dependency type
  keeps its package-qualified name whether it is an alias, a named `list`/`option`/`result`, or a
  member-bearing type; local aliases still expand. Because the recorded form cannot expose a
  foreign alias's target change, `check --against` now compares the pinned dependency graphs
  with the generators' shared rule when its baseline is a version directory's `wire.lock`. A
  copied baseline outside a version directory cannot be compared; `check` says so in a note and
  CI must run a generator over the root.
- `diff` now resolves a dependency alias of a named type (record, enum, variant, or flags) and
  defaultability through the package that declares it, even when the root defines a type with
  the same name. It also reads the `released` manifest beside a version-directory endpoint, so a
  malformed manifest or a symlink in a listed released directory is refused with exit `2` as
  `check` refuses it. `diff.md` and the emitter guide state both behaviours.
- `check` and `lock` now report every released-member discrepancy in one run instead of stopping
  at the first kind or type change: all kind/type changes, renumbering, un-reserving, missing
  entries, reintroductions, order changes, and mixed-kind parents are collected in lockfile key
  order. `lock` writes nothing when the list is non-empty. JSON and SARIF carry one diagnostic
  per discrepancy.
- The GitHub Action reads `diff-old`, `diff-new`, `diff-check`, and `github-token`. Every Action
  release through v0.5.0 looked them up under the wrong environment names and treated them as
  empty: a workflow that passed `diff-old` and `diff-new` ran `check` and the generators and
  never ran `witforge diff`, and latest-release lookups were anonymous. Workflows pinned to an
  earlier Action release should run `witforge diff` as a separate step. A workflow that already
  passes `diff-old` and `diff-new` starts running `witforge diff … --check` when its pin moves
  to `v0.6.0`, and then fails on any wire-, source-, or generated-API-breaking
  change, which includes an added record field; set `diff-check: "false"` to print the diff
  without failing. The token is sent only to the latest-release query, not with the asset
  downloads.
- The quickstart's first contract declared an interface and a world with one name, which WIT
  refuses, so its first command failed. The quickstart, README, and evolution guide now include
  the steps their evolution sequences omitted: creating the new version directory, copying the
  WIT, and changing the package version. The evolution guide no longer says that a field of a
  flags type is defaulted or that new-to-old conversion reports dropped fields.
- `diff` rejects a reversed old/new argument order and reports a missing `wire.lock` on a direct
  version-directory endpoint with the same actionable guidance as `check`.

## [0.5.0] - 2026-11-17

0.5.0 publishes the `diff` review improvements and the post-0.4.4 output and documentation
alignment. It adds an additive public `diff` API and new `diff` output; it does not change wire
numbers, protobuf output, JSON Schema output, or the generated Rust type and trait surface.
Generated `convert.rs` no longer ends with an extra blank line, so consumers that commit it
should regenerate when upgrading. Consumers moving from 0.4.3 or earlier also pick up the 0.4.4
generated-glue changes; regenerate committed glue as well as `convert.rs`.

Release commit `c5007a0` (tag `v0.5.0`), exact-commit main CI run 44034805107, release run
44035490910. Both crates are published; `cargo install witforge-cli --version 0.5.0 --locked`
reports `witforge 0.5.0`, docs.rs built the library, and the release carries four platform
archives with checksums. The tagged Action passed `check` and all four configured generator
checks on every release target.

### Added

- `diff` reports a guaranteed new-to-old conversion failure as an informational note when a
  shared record type's older version requires a member the newer version removed and that
  member's type has no default. Human, `--json`, `--format json`, and SARIF output carry the
  notes; they do not change compatibility counts or exit codes.

### Fixed

- Corrected the Component Model guide: wit-bindgen type remapping works for compatible records
  and variants, but not witforge enums, flags, or aggregates containing them with wit-bindgen
  0.63.0.
- Published the semantic decode rules non-Rust protobuf implementations must enforce, and
  documented the distinct plain-Rust Serde, WIT-shaped JSON Schema, and proto3-JSON projections.
- `diff` now refuses a stale lockfile on a direct version-directory endpoint instead of
  comparing stale entries and potentially reporting a compatible result.
- Removed the extra blank line at the end of generated `convert.rs`, and added a whole-tree CI
  whitespace check so committed output cannot preserve the same defect.
- Kept wide documentation tables inside the mobile viewport with local horizontal scrolling.
- Added direct guide and API-documentation links to the repository surface, and clarified that
  `witforge lock --first` creates the first version's `wire.lock`.
- Replaced generated-glue test rlib scanning with isolated Cargo compile/run harnesses, so clean
  hosted macOS jobs no longer depend on another test having compiled `prost` first.

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
