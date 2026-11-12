---
layout: default
title: Examples
---

# Examples

The repository contains two runnable examples with committed generated output.

## Single version

[`examples/single`](https://github.com/asamarts/witforge/tree/main/examples/single) exercises:

- one contract version;
- `witforge.toml`;
- Rust, protobuf, JSON Schema, and glue generation;
- committed output checked by every generator's `--check` mode.

```sh
git clone https://github.com/asamarts/witforge.git
cd witforge/examples/single
witforge check wit/v0.1
witforge rust --config witforge.toml
witforge proto --config witforge.toml
witforge schema --config witforge.toml
witforge glue --config witforge.toml
```

The exact generated files are under
[`examples/single/generated`](https://github.com/asamarts/witforge/tree/main/examples/single/generated).
The [generated-output guide](generated-output.html) walks through representative sections.

## Evolved contract

[`examples/evolved`](https://github.com/asamarts/witforge/tree/main/examples/evolved) contains released `v0.1` and draft `v0.2`. It demonstrates lock continuity, `diff`, release freezing, and generated version adapters.

```sh
cd witforge/examples/evolved
witforge check wit/v0.2 --against wit/v0.1/wire.lock
witforge diff wit/v0.1 wit/v0.2
witforge rust --config witforge.toml
```

The example adds defaultable fields. Version-adapter generation accepts them, but `diff --check`
still reports a source/generated-API break. Use plain `diff` to review the example; add `--check`
when project policy forbids those changes.

Generated output is committed for review and checked for drift in CI. Run any generator without
`--check` to update it, review the diff, then run the same command with `--check`.
