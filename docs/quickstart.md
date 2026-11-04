# witforge quickstart

witforge turns one hand-written [WIT](https://component-model.bytecodealliance.org/design/wit.html)
package into the artifacts the two sides of a contract need:

- plain Rust types and interface traits;
- a protobuf schema with append-only field numbers;
- Draft 7 JSON Schema definitions;
- glue between the plain Rust and protobuf types;
- old-to-new and new-to-old Rust adapters for adjacent contract versions.

It is not a runtime, transport, or WASM guest-binding generator. Non-Rust consumers use the
emitted `.proto` with their normal protobuf toolchain; Rust consumers can use the generated plain
types directly.

## Install

After the 0.2 release is published:

```sh
cargo install witforge-cli --version 0.2.0 --locked
witforge --version
```

From this repository, before or after publication:

```sh
cargo run -p witforge-cli -- --version
```

The examples below use `witforge`; replace it with `cargo run -p witforge-cli --` when working
from a checkout.

## One version

Create a root with a version directory:

```text
my-contract/
  wit/
    v0.1/
      contract.wit
      wire.lock
```

Start minimal:

```wit
package example:hello@0.1.0;

interface greeter {
    greet: func(name: string) -> string;
}

world greeter {
    export greeter;
}
```

Validate the contract and assign wire numbers:

```sh
witforge lock  my-contract/wit/v0.1 --first
witforge check my-contract/wit/v0.1
```

Commit `contract.wit` and `wire.lock`. The lockfile is append-only: generated protobuf field
numbers and method ids must never change.

Generate each projection:

```sh
witforge rust   my-contract/wit --out my-contract/generated/rust
witforge proto  my-contract/wit --out my-contract/generated/proto
witforge schema my-contract/wit --out my-contract/generated/schema
witforge glue   my-contract/wit --out my-contract/generated/glue \
  --plain-root crate::generated::rust --proto-root crate::generated::proto
```

`--plain-root` and `--proto-root` are Rust module paths, not output directories. The glue emitter
uses them to reference the plain types and the `prost` types generated from the `.proto` schema.

Every generator supports `--check`, which writes nothing and fails if committed output differs:

```sh
witforge rust   my-contract/wit --out my-contract/generated/rust --check
witforge proto  my-contract/wit --out my-contract/generated/proto --check
witforge schema my-contract/wit --out my-contract/generated/schema --check
witforge glue   my-contract/wit --out my-contract/generated/glue \
  --plain-root crate::generated::rust --proto-root crate::generated::proto --check
```

## Use `witforge.toml`

Put the repeated paths in a project file:

```toml
version = 1

[project.hello]
root = "wit"

[project.hello.rust]
out = "generated/rust"

[project.hello.proto]
out = "generated/proto"

[project.hello.schema]
out = "generated/schema"

[project.hello.glue]
out = "generated/glue"
plain-root = "crate::generated::rust"
proto-root = "crate::generated::proto"
```

Then each generator only needs the project:

```sh
witforge rust   --config witforge.toml
witforge proto  --config witforge.toml
witforge schema --config witforge.toml
witforge glue   --config witforge.toml
```

If the file has more than one project, add `--project NAME`. Paths inside the file are relative to
the file itself. Explicit CLI paths override configured paths, and the command-line-only form
continues to work.

## Evolve a contract

Keep every released version in the tree:

```text
wit/
  released              # contains v0.1
  v0.1/
    contract.wit
    wire.lock
  v0.2/
    contract.wit
    wire.lock
```

Start the new minor from the previous lockfile:

```sh
cp wit/v0.1/wire.lock wit/v0.2/wire.lock
witforge lock  wit/v0.2 --against wit/v0.1/wire.lock
witforge check wit/v0.2 --against wit/v0.1/wire.lock
```

Add new members with defaultable types when possible:

```wit
record task {
    id: u64,
    title: string,
    done: bool,
    priority: option<u8>,   // old values become None
    tags: list<string>,     // old values become an empty list
}
```

`witforge rust` emits `convert.rs` with old-to-new and new-to-old adapters. Missing required older
members are reported as `ConversionError::MissingMember`; added or removed cases are reported as
`ConversionError::UnknownCase`. A required, non-defaultable addition is refused by conversion
generation.

`wit/released` lists frozen versions. Do not edit a released version's WIT, lockfile, or generated
Rust module. The release-freeze check and `--check` enforce that rule.

## Non-Rust consumers

Use the protobuf output with the normal generators for your language:

- Go: `protoc-gen-go` / `prost`-equivalent tooling;
- TypeScript: `ts-proto` or another protobuf generator;
- Python: `protoc`/`grpcio-tools` or another protobuf generator;
- Java, Kotlin, C#, C++, Swift, and others: their standard protobuf generators.

witforge intentionally does not duplicate `wit-bindgen`'s runtime WIT bindings. The stable
protobuf numbers and JSON Schema are the cross-language contract surface.

## Supported WIT subset

The current subset is deliberately pure and synchronous:

- package versions are `<major>.<minor>.0`;
- types and functions live in named interfaces;
- records, variants, enums, flags, aliases, `use` aliases, lists, options, and results are
  supported;
- resources, maps, async, futures, streams, tuples, chars, fixed-length lists, foreign packages,
  and world-defined types are currently refused.

Run `witforge check` for the exact diagnostic. `docs/design/overview.md` has the full contract
model and the reasons for each refusal.

## Examples

- [`examples/single`](../examples/single/) — one version, all four generators, config-driven.
- [`examples/evolved`](../examples/evolved/) — released v0.1 plus draft v0.2, lock continuity and
  generated conversions.

## Troubleshooting

- **`wire.lock` missing**: for the first version use `witforge lock <dir> --first`; for a new
  minor copy the previous lockfile and use `--against`.
- **stale lockfile**: run `witforge lock`, review the printed `added`, `reserved`, `updated`, and
  `dropped` entries, and commit the result.
- **released version changed**: restore the released WIT and lockfile; create a new version
  directory for the change.
- **glue module path errors**: pass the Rust module roots, not filesystem paths.
- **generated output drift**: run the generator without `--check`, review the diff, and commit
  the result.
