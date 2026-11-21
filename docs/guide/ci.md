---
layout: default
title: Continuous integration
---

# Continuous integration

## GitHub Action

Pin the Action to the same release used for local generation, and run the contract diff as its
own step. The Action leaves `witforge` on `PATH`:

```yaml
- uses: asamarts/witforge@v0.6.0
  with:
    check: wit/v0.2
    config: witforge.toml
    generators: rust,proto,schema,glue
- run: witforge diff wit/v0.1 wit/v0.2
```

The Action installs a checksum-verified release binary and runs configured generators with
`--check`. The `diff` step prints what changed between the two versions.

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
- `diff-check: "false"` prints the diff without failing the job.

A workflow that already passes `diff-old` and `diff-new` therefore starts gating when its pin
moves to `v0.6.0` or later. Set `diff-check: "false"` before moving the pin if the job should keep
passing on source-breaking changes.

## Direct commands

```sh
witforge check wit/v0.2 --against wit/v0.1/wire.lock
witforge diff wit/v0.1 wit/v0.2 --check
witforge rust --config witforge.toml --check
witforge proto --config witforge.toml --check
witforge schema --config witforge.toml --check
witforge glue --config witforge.toml --check
```

Run both `diff --check` and at least one generator over the whole WIT root. `check` validates one package version; whole-root generation also checks dependency-graph continuity.

## Consumer tests

Generated-file checks prove reproducibility, not application behavior. Also compile generated Rust, compile the emitted protobuf with the consumer's normal toolchain, round-trip representative values, and exercise application adapters and defaults.

For a JSON protocol, commit full and minimal fixtures and run them through every language implementation.
