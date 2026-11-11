---
layout: default
title: Continuous integration
---

# Continuous integration

## GitHub Action

Pin the Action to the same release used for local generation:

```yaml
- uses: asamarts/witforge@v0.4.3
  with:
    check: wit/v0.2
    diff-old: wit/v0.1
    diff-new: wit/v0.2
    config: witforge.toml
    generators: rust,proto,schema,glue
```

The Action installs a checksum-verified release binary and runs configured generators with `--check`.

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
