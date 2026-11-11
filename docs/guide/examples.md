---
layout: default
title: Examples
---

# Examples

The repository contains two runnable examples.

## Single version

[`examples/single`](https://github.com/asamarts/witforge/tree/main/examples/single) exercises:

- one contract version;
- `witforge.toml`;
- Rust, protobuf, JSON Schema, and glue generation;
- every generator's `--check` mode.

```sh
git clone https://github.com/asamarts/witforge.git
cd witforge/examples/single
witforge check wit/v0.1
witforge rust --config witforge.toml
witforge proto --config witforge.toml
witforge schema --config witforge.toml
witforge glue --config witforge.toml
```

## Evolved contract

[`examples/evolved`](https://github.com/asamarts/witforge/tree/main/examples/evolved) contains released `v0.1` and draft `v0.2`. It demonstrates lock continuity, `diff`, release freezing, and generated version adapters.

```sh
cd witforge/examples/evolved
witforge check wit/v0.2 --against wit/v0.1/wire.lock
witforge diff wit/v0.1 wit/v0.2 --check
witforge rust --config witforge.toml
```

Generated output is ignored in the examples so the commands can be run repeatedly in a scratch checkout.
