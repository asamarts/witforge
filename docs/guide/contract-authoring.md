---
layout: default
title: Contract authoring
---

# Contract authoring

## Layout

```text
wit/
  released
  v0.1/
    contract.wit
    wire.lock
  v0.2/
    contract.wit
    wire.lock
```

A directory named `vX.Y` contains WIT package version `X.Y.0`. Released versions are immutable.

## First version

To scaffold `wit/v0.1/contract.wit`, an initial `wire.lock`, and `witforge.toml` in one command,
run `witforge init <dir> --package example:checker`. To lock and validate an existing directory by
hand:

```sh
witforge lock wit/v0.1 --first
witforge check wit/v0.1
```

Commit both WIT and `wire.lock`.

## WIT example

```wit
package example:checker@0.1.0;

interface api {
  record request {
    text: string,
  }

  record diagnostic {
    start: u32,
    end: u32,
    message: string,
  }

  check: func(input: request) -> list<diagnostic>;
}

world plugin {
  export api;
}
```

Use named records instead of tuples and explicit IDs/records instead of resources when the contract must cross Rust, protobuf, and JSON Schema.

## Supported types, stability annotations, and inspection

The supported subset includes records, variants, enums, flags, aliases, `use` aliases, `char`,
lists, fixed-length lists (`list<T, N>`, `1 <= N <= 65536`), primitive-keyed maps (`map<K, V>`
with `bool`, integer, `char`, or `string` keys), options, results, named interface functions, and
worlds.

Standard WIT stability annotations (`@since`, `@unstable`, and `@deprecated`) propagate into
generated Rust docs/attributes, protobuf comments/options, JSON Schema metadata, and generated
Markdown docs. Set `[project.<name>.policy].forbid-unstable-in-release = true` (or pass
`witforge release --forbid-unstable`) to block releasing `@unstable` items.

Run `witforge inspect wit` for a concise contract summary, or `witforge inspect wit --json` to
export the deterministic, versioned JSON contract IR (`version: 1`) for downstream tools.

## Dependencies

Place standard WIT dependencies under each version's `deps/` directory. All versions within one
major must resolve an identical dependency graph (use `witforge deps sync` / `witforge deps sync --check`
to keep `deps/` in sync across unreleased versions of a major without touching released versions);
across majors, dependencies may be upgraded or changed and are scoped per major. witforge emits
referenced dependency bindings and glue with package-qualified identity.

See the [dependency specification](https://github.com/asamarts/witforge/blob/main/docs/design/dependencies.md) for layout and generated modules.
