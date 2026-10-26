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

> **Status: pre-release, private.** The first slice is implemented: loading a WIT package with
> every feature enabled, the restricted-subset checker, and the append-only wire lockfile, with
> `witforge check` and `witforge lock`. The internal model, v2 lockfile format, and Rust/protobuf
> emitter conventions are specified in
> [docs/design/model.md](docs/design/model.md),
> [docs/design/lockfile.md](docs/design/lockfile.md), and
> [docs/design/emitters.md](docs/design/emitters.md); implementation comes next, then the
> emitters themselves. See [ROADMAP.md](ROADMAP.md). witforge's first consumer is the
> [alint](https://github.com/asamarts/alint) plugin contract.

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

$ witforge check wit/v0.2 --against wit/v0.1/wire.lock   subset + continuity   (CI)
$ witforge lock  wit/v0.2 --against wit/v0.1/wire.lock   number new members
$ witforge lock  wit/v0.1 --first                        bootstrap a version
```

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

The design is in [docs/design/overview.md](docs/design/overview.md).

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT), at your option.
