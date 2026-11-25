# Contributing to witforge

Thank you for your interest in contributing to witforge! This guide covers the local verification
gates, scratch-space setup, golden-file and example updates, how task planning works with `aplan`,
and the immutability rules for versioned contracts.

## Local verification gates

Before opening a pull request, run the workspace gates on stable Rust (and, if installed, on the
supported MSRV **1.88**):

```sh
mkdir -p "$HOME/.tmp"
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy -p witforge-build --all-targets --all-features -- -D warnings
RUSTDOCFLAGS='-D warnings' cargo doc --workspace --all-features --no-deps
TMPDIR="$HOME/.tmp" cargo test --workspace --locked
TMPDIR="$HOME/.tmp" cargo test -p witforge-build --all-features --locked
TMPDIR="$HOME/.tmp" cargo +1.88 test --workspace --locked
TMPDIR="$HOME/.tmp" cargo +1.88 test -p witforge-build --all-features --locked
```

Also run the fuzz-manifest, Action, and repository-script checks when touching those areas:

```sh
cargo fmt --manifest-path fuzz/Cargo.toml --check
cargo check --manifest-path fuzz/Cargo.toml --locked
node --test action/test.js
cargo build --locked -p witforge-cli
WITFORGE="$PWD/target/debug/witforge" ci/check-generated.sh
WITFORGE="$PWD/target/debug/witforge" ci/check-release-freeze.sh crates/witforge/tests/fixtures/versions/wit
WITFORGE="$PWD/target/debug/witforge" ci/check-release-freeze.sh examples/evolved/wit
ci/check-release.sh
```

CI runs `git diff --check` from Git's empty tree to `HEAD`, so ensure there is no trailing
whitespace or extra blank lines at end-of-file, and all generated/committed text files use LF line
endings.

## Scratch space (`TMPDIR`)

Rust integration tests (`std::env::temp_dir()`) compile temporary consumer crates against `prost`
and `protox`, and every `ci/*.sh` script writes temporary trees under `${TMPDIR:-$HOME/.tmp}`. On
Linux hosts where `/tmp` is a memory-backed `tmpfs`, always point `TMPDIR` at a disk-backed
directory such as `$HOME/.tmp`:

```sh
mkdir -p "$HOME/.tmp"
TMPDIR="$HOME/.tmp" cargo test --workspace --locked
```

Tests automatically delete their scratch directories on success and retain them (printing the path
to stderr) when a failure occurs for inspection.

## Contract evolution rule

A contract change is **always a new version directory** (`v<major>.<minor>`), never an in-place
edit to a released directory listed in `wit/released`. Released WIT files, `wire.lock` files,
dependency packages, and generated per-version Rust modules are immutable and enforced by
`witforge freeze` across Git history. See the [Contract evolution guide](docs/guide/evolution.md)
and [Lockfile specification](docs/design/lockfile.md).

## Updating golden files and committed examples

Golden files are **not** overwritten automatically by an environment variable; changes to
generated output are intentional contract changes and must be reviewed explicitly:

1. **Committed runnable examples (`examples/single` and `examples/evolved`):** regenerate them with
   the local CLI, inspect `git diff`, and verify `--check` and compilation:

   ```sh
   cargo run -p witforge-cli -- generate --config examples/single/witforge.toml
   cargo run -p witforge-cli -- generate --config examples/evolved/witforge.toml
   WITFORGE="$PWD/target/debug/witforge" ci/check-generated.sh
   ```

2. **Emitter and model goldens (`crates/witforge/tests/fixtures/emit/` and
   `crates/witforge/tests/fixtures/model/members.golden`):** run the relevant test (for example
   `TMPDIR="$HOME/.tmp" cargo test -p witforge --test emit_rust`), inspect the diff reported by the
   failing assertion, update the committed golden file deliberately, and re-run the test suite
   (including `compile_matrix` and `distribution` snippet checks in `crates/witforge-cli/tests/distribution.rs`).
3. **Documentation excerpts and migration notes:** if generated output changed for existing
   contracts, update any affected excerpts in `README.md` or `docs/guide/generated-output.md`,
   and record what changed (and how consumers can tell whether they are affected) under
   `## [Unreleased]` in [`CHANGELOG.md`](CHANGELOG.md).

## Planning and status (`aplan`)

Task-level planning, dependencies, and closure evidence live under [`aplan/tasks/`](aplan/README.md)
and are checked in CI by:

```sh
aplan check
aplan verify
aplan export-agents-md --check AGENTS.md
aplan check-docs
```

- Never hand-edit `status: done`, `evidence`, or `trace_fingerprints` in `aplan/tasks/*.md`; use
  `aplan start`, `aplan close`, and `aplan close --restamp`.
- **Contributors without `aplan` installed:** you do not need to install `aplan` to open an issue
  or pull request. Describe the bug, design change, or fix clearly in the PR body (and cite the
  existing `W<phase>-<nnn>` task ID if you are working on one). A maintainer will create or update
  the `aplan` task and run the `aplan` closure gates before merging.

## Code of conduct

This repository does not currently adopt a separate `CODE_OF_CONDUCT.md` file (to be revisited as
external contributor volume grows). Contributors and maintainers are expected to keep all issues,
reviews, and discussions respectful, technical, and constructive under GitHub's Community
Guidelines.

## Reporting security issues

Do **not** open a public issue or pull request for a suspected security vulnerability. Report
vulnerabilities privately through
[GitHub Private Vulnerability Reporting](https://github.com/asamarts/witforge/security/advisories/new);
see [`SECURITY.md`](SECURITY.md).
