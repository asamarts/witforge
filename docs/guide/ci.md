---
layout: default
title: Continuous integration
---

# Continuous integration

## GitHub Action

Check out with full Git history (`fetch-depth: 0`) so `witforge freeze` can inspect the history of
`wit/released`, pin the Action to a release tag or commit SHA, and let the Action run `check`,
`diff`, `freeze`, and every configured generator with `--check`.

### Passing workflow on `examples/evolved`

`examples/evolved` adds defaultable fields in `v0.2` (wire-compatible, source-breaking for Rust
struct literals). Setting `diff-check: "wire"` verifies wire compatibility while allowing the
additive minor evolution; this exact combination passes on `examples/evolved`:

```yaml
- uses: actions/checkout@v4
  with:
    fetch-depth: 0
- uses: asamarts/witforge@v0.9.0
  with:
    check: examples/evolved/wit/v0.2
    against: examples/evolved/wit/v0.1/wire.lock
    diff-old: examples/evolved/wit/v0.1
    diff-new: examples/evolved/wit/v0.2
    diff-check: wire
    config: examples/evolved/witforge.toml
    generators: rust,proto,schema,glue
    freeze: "true"
    # Optional from v0.8.0 (defaults to github.token); pass explicitly on <= v0.7.x.
    github-token: ${{ github.token }}
```

The Action validates every input before downloading anything, downloads a checksum-verified (and
from 0.7.0, build-provenance-verified) release archive, removes the downloaded archive/checksum
scratch files, leaves `witforge` on `PATH` (`WITFORGE_BIN`), and emits GitHub workflow annotations
(`::error file=...,line=...,col=...::...`) whenever a check fails.

### Failing compatibility gate and SARIF upload

To fail a pull request on any source-, generated-API-, wire-, or protobuf binary decode break, set
`diff-check: "true"` (default: `wire,source,api`) or `diff-check: "wire,decode"`. Set
`format: sarif` to write a combined SARIF 2.1.0 report and upload it in a separate step:

```yaml
permissions:
  contents: read
  security-events: write

steps:
  - uses: actions/checkout@v4
    with:
      fetch-depth: 0
  - id: witforge
    uses: asamarts/witforge@v0.9.0
    with:
      check: wit/v0.2
      against: wit/v0.1/wire.lock
      diff-old: wit/v0.1
      diff-new: wit/v0.2
      diff-check: wire,decode
      config: witforge.toml
      generators: rust,proto,schema,glue
      freeze: "true"
      format: sarif
      sarif-file: witforge.sarif
      # Optional from v0.8.0 (defaults to github.token); pass explicitly on <= v0.7.x.
      github-token: ${{ github.token }}
  - if: always() && steps.witforge.outputs.sarif-file != ''
    uses: github/codeql-action/upload-sarif@v3
    with:
      sarif_file: ${{ steps.witforge.outputs.sarif-file }}
```

Uploading SARIF requires `security-events: write` and is unavailable to pull requests from forks
with read-only tokens. Workflow annotations require no extra permissions and always appear on pull
request diffs regardless of `format`.

### Version pinning and SHA pins

- When `version` is empty and the Action ref is a release tag (`@v0.9.0`), the Action installs that
  exact release.
- When `version` is empty and the Action ref is a commit SHA, branch, floating tag, or local `./`,
  the Action installs the workspace version recorded in `Cargo.toml` at that exact ref; it never
  silently falls back to `latest`. If that version has no published release assets yet, the step
  fails and names the `version` input.
- Pass `version: latest` explicitly only if you intentionally want floating latest releases.
- Pass `binary: path/to/witforge` to skip download and run a locally built executable (cannot be
  combined with `version`).

### Action inputs

| Input | Default | Description |
|---|---|---|
| `version` | empty | `X.Y.Z`, `vX.Y.Z`, or `latest`. Empty installs the release tag or the workspace version at the Action's commit SHA/branch. Cannot be combined with `binary`. |
| `binary` | empty | Path to an existing local `witforge` binary to run instead of downloading a release. Cannot be combined with `version`. |
| `check` | empty | WIT version directories or WIT roots to pass to `witforge check`, one per line, or `true` (from `v0.8.0`) to check every project in `config`. |
| `against` | empty | Baseline `wire.lock` path(s) for `witforge check --against`: a single lockfile path when `check` lists one directory, or `<check-dir>=<baseline-lockfile>` lines when `check` lists multiple directories. Cannot be combined with `check: true`. |
| `diff-old` | empty | Old version directory or WIT root for `witforge diff` (must be paired with `diff-new`). |
| `diff-new` | empty | New version directory or WIT root for `witforge diff` (must be paired with `diff-old`). |
| `diff-check` | `true` | `true` (`--check`, gates on `wire,source,api`), `false` (report only), or comma-separated views from `wire`, `source`, `api`, `decode` (`--check=<views>`). |
| `config` | empty | Path to `witforge.toml` for `check: true`, `freeze: true`, and configured generators. |
| `project` | empty | Optional project name in `witforge.toml`. |
| `generators` | `rust` | Comma- or whitespace-separated subset of `rust`, `proto`, `schema`, `glue`, `doc` (from `v0.9.0`) run with `--config` and `--check`, or `all` (from `v0.8.0`) to run `witforge generate --config ... --check` across every configured generator and project. Empty skips generation (pass `generators: ""` when using `config` only for `check: true` or `freeze: true`). |
| `freeze` | empty | WIT roots to verify with `witforge freeze` (one per line), or `true` to verify every project in `config`. Requires `fetch-depth: 0`. |
| `format` | `human` | Output format: `human`, `json`, or `sarif`. Failing checks emit GitHub workflow annotations in all formats. |
| `sarif-file` | empty | Output file path for the combined SARIF 2.1.0 report when `format: sarif` is set (defaults to `witforge.sarif`). |
| `github-token` | `${{ github.token }}` (from `v0.8.0`) | Token sent only to GitHub API calls (`version: latest` lookup and `gh attestation verify`), defaulting to the workflow token from `v0.8.0` (pass explicitly on `<= v0.7.x`) and never sent with asset downloads. |

### Action outputs

| Output | Description |
|---|---|
| `failed-command` | Empty on success; names the failing stage (`validate`, `install`, `sarif`, or `witforge <subcommand>`, such as `witforge diff`) on failure. |
| `sarif-file` | Path to the combined SARIF 2.1.0 file when `format: sarif` was requested. |

### Release freeze and `diff` upgrade notes

`witforge freeze` refuses shallow clones with exit code `2` and names `fetch-depth: 0`; never skip
it with a shallow checkout. It protects committed WIT contract sources and `wire.lock` files under
every released directory. Generated per-version modules (such as `v0_1.rs`) are frozen by
committing the generated copies and verifying them with `--check`, not by `witforge freeze`.

**Action releases up to and including v0.5.0 ignored `diff-old`, `diff-new`, `diff-check`, and
`github-token`** because they read them under underscore environment names. Fixed in `v0.6.0`:
workflows already passing `diff-old` and `diff-new` begin running `witforge diff --check` when
moving to `v0.6.0` or later; set `diff-check: "false"` or `diff-check: "wire"` if source-breaking
additions should not fail the job.

## Direct commands

```sh
witforge freeze --config witforge.toml
witforge check wit
witforge diff wit/v0.1 wit/v0.2 --check
witforge generate --config witforge.toml --check
```

Recommended direct CI gates:

- `witforge freeze --config witforge.toml` (with `fetch-depth: 0`) to verify released history and
  one-way retirement transitions;
- `witforge check <wit-root>` to check every version directory, predecessor lockfile continuity,
  and the pinned dependency graph;
- `witforge diff --git-base origin/main --check` (or `witforge diff old new --check`), which uses
  `[project.<name>.policy].diff-check` when configured in `witforge.toml` (set
  `forbid-unstable-in-release = true` in the same policy table to block releasing `@unstable`
  items);
- `witforge deps sync --check` when vendored `deps/` are shared across unreleased minors;
- `witforge generate --config witforge.toml --check` to verify Rust, protobuf, JSON Schema, glue,
  and Markdown docs (`witforge doc`) have not drifted.

## Consumer tests

Generated-file checks prove reproducibility, not application behavior. Also compile generated Rust,
compile the emitted protobuf with the consumer's normal toolchain, round-trip representative
values, and exercise application adapters and defaults.

For a JSON protocol, commit full and minimal fixtures and run them through every language
implementation.
