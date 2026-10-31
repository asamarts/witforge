# witforge

**A WIT-first contract compiler.** Write a contract once, as a
[WIT](https://component-model.bytecodealliance.org/design/wit.html) package, and generate the
bindings the other sides of the contract need:

- plain Rust types and traits (no wasmtime or component-runtime dependency), for native
  implementations and for hosts;
- a protobuf schema with **stable field numbers**, for out-of-process implementations in any
  language;
- later, JSON Schema.

WIT is the strictest of these type systems (the component model treats a published record,
variant, enum, or flags type as frozen), so projecting it onto Rust and protobuf loses nothing,
while the reverse directions are lossy. witforge exists because no tool performs that
projection: the ecosystem's converters go from protobuf to WIT, and the WIT bindings generators
tie their output to a runtime.

> **Status: 0.1.0.** The loading, subset-checker, internal-model, v2 append-only lockfile, Rust,
> protobuf, and glue emitters are implemented, with clap-based `witforge check`, `lock`, `rust`,
> `proto`, and `glue` commands. CI runs on Linux, macOS, and Windows and gates generated-output
> drift and release freezes. witforge's first consumer is the
> [alint](https://github.com/asamarts/alint) plugin contract.

## Install

```sh
cargo install witforge-cli --locked
```

This installs the `witforge` command; the `witforge` crate is the library behind it. Both need
Rust 1.88 or later.

## Plan

Task-level planning, dependencies, and closure evidence live in [`aplan/`](https://github.com/asamarts/witforge/blob/main/aplan/README.md)
and are validated with `aplan check` and `aplan verify` (aplan is not yet published, so CI does
not run them). [`ROADMAP.md`](https://github.com/asamarts/witforge/blob/main/ROADMAP.md) is the narrative sequence; the plan is the
task-level source of truth.

## How it works

```text
wit/                      hand-written contracts (the source of truth)
  released                released version directories
  v0.1/
    *.wit
    wire.lock             append-only protobuf numbers, committed
  v0.2/
    *.wit
    wire.lock

$ witforge lock  wit/v0.1 --first                         number the first version
$ cp wit/v0.1/wire.lock wit/v0.2/wire.lock                start a new minor from the last lock
$ witforge lock  wit/v0.2 --against wit/v0.1/wire.lock   number its new members
$ witforge check wit/v0.2 --against wit/v0.1/wire.lock   subset + continuity   (CI)
$ witforge rust  wit --out src/plain                     plain Rust modules
$ witforge proto wit --out proto                         protobuf schema
$ witforge glue  wit --out src/glue --plain-root crate::plain --proto-root crate::pb
```

`--proto-root` names the Rust module that holds the prost types generated from the emitted
schema (for example by `prost-build` in a build script), not the directory the schema is
written to. Every generated file starts with witforge's header, and `--check` on any generator
fails instead of writing when its output would change.

- **The subset.** Contracts stay inside a pure, synchronous, self-contained subset of WIT: no
  resources or handles, no `future`, `stream`, or `error-context`, no `map`, no fixed-length
  lists, no `char`, no tuples, no `async` functions or `get`/`set` accessors, nothing from
  another package, and package versions are `<major>.<minor>.0`. Types and functions live in
  the package's named interfaces, which worlds import and export. Anything unrecognised is
  refused, never passed through.
- **The lockfile.** WIT members carry no field numbers, so `wire.lock` maps every record field,
  variant case, enum case, flag, function method id, and function parameter to a number that
  never changes. New members get the next free number in their parent; removed members stay,
  marked `reserved`, and their names cannot come back; nothing is renumbered or deleted, and the
  package version is not part of the key, so version bumps never renumber anything. Entries also
  record their kind and canonical type, so a type or kind change is caught even when protobuf
  would not see it.

The design is in [docs/design/overview.md](https://github.com/asamarts/witforge/blob/main/docs/design/overview.md).

## License

Licensed under either of [Apache License, Version 2.0](https://github.com/asamarts/witforge/blob/main/LICENSE-APACHE) or
[MIT license](https://github.com/asamarts/witforge/blob/main/LICENSE-MIT), at your option.
