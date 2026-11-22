---
layout: default
title: Continuous integration
---

# Continuous integration

## GitHub Action

Check out with full Git history (`fetch-depth: 0`) so `witforge freeze` can inspect the history of
`wit/released`, pin the Action to the same release used for local generation, and run the contract
diff as its own step. The Action leaves `witforge` on `PATH`:

```yaml
- uses: actions/checkout@v4
  with:
    fetch-depth: 0
- uses: asamarts/witforge@v0.6.0
  with:
    check: wit/v0.2
    config: witforge.toml
    generators: rust,proto,schema,glue
    freeze: "true"
- run: witforge diff wit/v0.1 wit/v0.2
```

The Action installs a checksum-verified release binary, verifies that released directories listed
in `wit/released` have not changed since their release (`witforge freeze`), and runs configured
generators with `--check`. The `diff` step prints what changed between the two versions.

`witforge freeze` refuses shallow clones with exit code `2` and names `fetch-depth: 0`; never
skip it with a shallow checkout. It protects the committed WIT contract sources and `wire.lock`
files under every released directory. Generated per-version modules (such as `v0_1.rs`) are frozen
by committing the generated copies and verifying them with `--check`, not by `witforge freeze`.

Add `--check` to the `diff` step to fail the job on any wire-, source-, or generated-API-breaking
change. An added record field is source-breaking even when its type is defaultable, so this gate
fails for most minors; use it where a minor must not break the code of the contract's consumers.

### The Action's diff inputs

The Action can run the diff itself through `diff-old`, `diff-new`, and `diff-check`.

**Action releases up to and including v0.5.0 ignored these three inputs and `github-token`.** They
read them under the wrong environment names, so a workflow that passed `diff-old` and `diff-new`
ran `check` and the generators and never ran `diff`. Fixed in `v0.6.0`. From `v0.6.0` on:

- `diff-old` and `diff-new` run `witforge diff <old> <new> --check`, because `diff-check`
  defaults to `true`;
- `diff-check: "false"` prints the diff without failing the job;
- `diff-check: "wire"` (or another comma-separated subset of `wire`, `source`, `api`) passes
  `--check=<views>` to gate only on those compatibility views.

A workflow that already passes `diff-old` and `diff-new` therefore starts gating when its pin
moves to `v0.6.0` or later. Set `diff-check: "false"` or `diff-check: "wire"` before moving the
pin if the job should keep passing on source-breaking changes.

## Direct commands

```sh
witforge freeze --config witforge.toml
witforge check wit
witforge diff wit/v0.1 wit/v0.2 --check
witforge generate --config witforge.toml --check
```

Run `witforge freeze` (with full Git history), `witforge check <wit-root>` (which checks every
version directory, predecessor lockfile continuity, and the pinned dependency graph), `diff --check`
(or `--check=wire`), and `witforge generate --check` (keeping generated per-version modules frozen
alongside the released WIT directories).

## Consumer tests

Generated-file checks prove reproducibility, not application behavior. Also compile generated Rust, compile the emitted protobuf with the consumer's normal toolchain, round-trip representative values, and exercise application adapters and defaults.

For a JSON protocol, commit full and minimal fixtures and run them through every language implementation.
