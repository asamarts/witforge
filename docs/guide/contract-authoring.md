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

## Dependencies

Place standard WIT dependencies under each version's `deps/` directory. All versions in one generated root must resolve an identical dependency graph. witforge emits referenced dependency bindings and glue with package-qualified identity.

See the [dependency specification](https://github.com/asamarts/witforge/blob/main/docs/design/dependencies.md) for layout and generated modules.
