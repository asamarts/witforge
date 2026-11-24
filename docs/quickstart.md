---
layout: default
title: Quickstart
---

# witforge quickstart

witforge turns one hand-written [WIT](https://component-model.bytecodealliance.org/design/wit.html)
package into the artifacts the two sides of a contract need:

- plain Rust types and interface traits;
- a protobuf schema with append-only field numbers;
- JSON Schema definitions (Draft 7 default, opt-in Draft 2020-12);
- glue between the plain Rust and protobuf types;
- old-to-new and new-to-old Rust adapters for adjacent contract versions.

It is not a runtime, transport, or WASM guest-binding generator. Non-Rust consumers use the
emitted `.proto` with their normal protobuf toolchain; Rust consumers can use the generated plain
types directly.

## Install

Install the current release:

```sh
cargo install witforge-cli --version 0.7.0 --locked
witforge --version
```

Prebuilt archives (with `.sha256` checksums, `LICENSE-APACHE`, `LICENSE-MIT`, `README.md`, and
from 0.7.0 SLSA build provenance attestations) are published on GitHub Releases for Linux x86_64
and Linux arm64 (static `musl` from 0.8.0 alongside `gnu`), macOS Intel and Apple silicon, and
Windows x86_64; `cargo binstall witforge-cli` is also configured. Windows arm64 builds are not
provided — use `cargo install witforge-cli --locked` on that platform (or see the
[wiring and installation guide](guide/wiring.md) for manual download and verification commands).

From this repository, before or after publication:

```sh
cargo run -p witforge-cli -- --version
```

The examples below use `witforge`; replace it with `cargo run -p witforge-cli --` when working
from a checkout. Every command on this page runs from one working directory, the one that
contains `my-contract/`, and the test suite runs them in this order.

## One version

To scaffold a starter project (`wit/v0.1/contract.wit`, initial `wire.lock`, and `witforge.toml`)
in one step without overwriting existing files, run `witforge init my-contract --package example:hello`
(add `--serde` to enable Serde derives). The manual steps below walk through each file explicitly.

Create a root with a version directory:

```text
my-contract/
  wit/
    v0.1/
      contract.wit
```

```sh
mkdir -p my-contract/wit/v0.1
```

Save a small contract as `my-contract/wit/v0.1/contract.wit`. The package version must match the
directory name, and an interface and a world cannot share a name:

```wit
package example:hello@0.1.0;

interface greeter {
    record greeting {
        text: string,
    }

    greet: func(name: string) -> greeting;
}

world hello {
    export greeter;
}
```

At this point, `contract.wit` is the only file in `v0.1`. Do not create `wire.lock` by hand.

Create the lockfile, assign wire numbers, and validate the contract:

```sh
witforge lock my-contract/wit/v0.1 --first
witforge check my-contract/wit/v0.1
```

`witforge lock --first` creates `my-contract/wit/v0.1/wire.lock`. Commit `contract.wit` and the
generated `wire.lock` together. The lockfile is append-only: generated protobuf field numbers and
method ids must never change.

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

Put the repeated paths in a project file, `my-contract/witforge.toml`:

```toml
version = 1

[project.hello]
root = "wit"

[project.hello.rust]
out = "generated/rust"
# serde = true  # opt in to Serialize/Deserialize derives

[project.hello.proto]
out = "generated/proto"

[project.hello.schema]
out = "generated/schema"

[project.hello.glue]
out = "generated/glue"
plain-root = "crate::generated::rust"
proto-root = "crate::generated::proto"
```

Then run every configured generator in one command (or invoke `rust`, `proto`, `schema`, and `glue` individually):

```sh
witforge generate --config my-contract/witforge.toml
witforge generate --config my-contract/witforge.toml --check
```

Inside `my-contract/` (or any subdirectory below it within the repository), `witforge.toml` is discovered automatically without passing `--config` (pass `--no-config` to disable discovery). If the file has more than one project, `generate` runs all of them unless `--project NAME` selects one. Paths inside the file are relative to the file itself. Explicit CLI paths override configured paths, and the command-line-only form continues to work. `--serde` or `--no-serde` overrides the Rust section's serde setting; the generated code then requires the consumer's `serde` crate with `derive`.

## Evolve a contract

A contract change is a new version directory, never an edit to a released one. Keep every
released version in the tree:

```text
my-contract/wit/
  released              # lists v0.1
  v0.1/
    contract.wit
    wire.lock
  v0.2/
    contract.wit
    wire.lock
```

When `v0.1` ships, validate and record it as released in `my-contract/wit/released`:

```sh
witforge release my-contract/wit/v0.1
```

Start the next minor from the latest version. `new-version` copies every `.wit` file, `deps/`, and
`wire.lock`, and updates the `package` version header to `0.2.0` (add `--major` for a new major
with no copied lockfile; the manual equivalent is `mkdir`, `cp` of the `.wit` files and `wire.lock`,
and bumping the `package` header yourself):

```sh
witforge new-version my-contract/wit
```

Edit `my-contract/wit/v0.2/contract.wit` to add the new members (using defaultable types when
possible so generated adapters can convert old values):

```wit
package example:hello@0.2.0;

interface greeter {
    record greeting {
        text: string,
        language: option<string>,   // old values become None
        tags: list<string>,         // old values become an empty list
    }

    greet: func(name: string) -> greeting;
}

world hello {
    export greeter;
}
```

Assign numbers to the new members, then validate the whole WIT root (which checks released
directories in default mode and every draft against its predecessor):

```sh
witforge lock  my-contract/wit/v0.2 --against my-contract/wit/v0.1/wire.lock
witforge check my-contract/wit
```

Regenerate everything. With two versions in the root, `rust` also writes `convert.rs`:

```sh
witforge generate --config my-contract/witforge.toml
witforge generate --config my-contract/witforge.toml --check
```

`convert.rs` holds old-to-new and new-to-old adapters. Old-to-new conversion fills an added
`option` with `None` and an added `list` with an empty list. New-to-old conversion drops members
the older version does not have. Missing required older members are reported as
`ConversionError::MissingMember`; added or removed cases are reported as
`ConversionError::UnknownCase`. A required, non-defaultable addition is refused by conversion
generation.

Do not edit a released version's WIT, lockfile, or generated Rust module. `witforge lock` refuses
a released directory, `check` and the generators fail when a released WIT file no longer matches
its lockfile, and `witforge freeze` verifies in Git history (with `fetch-depth: 0`) that no
released directory was modified or dropped from `wit/released`.

## Review a change

Compare a draft with the previous version before merging it:

```sh
witforge diff my-contract/wit/v0.1 my-contract/wit/v0.2
witforge diff my-contract/wit/v0.1 my-contract/wit/v0.2 --json
witforge diff my-contract/wit/v0.1 my-contract/wit/v0.2 --check   # exits 1: see below
```

`--check` fails on wire-, source-, or generated-API-breaking changes. It exits 1 for the change
above: the two added fields keep every wire number and convert from old values, but Rust code
that constructs a `Greeting` has to be updated, so `diff` reports them as source- and
API-breaking. `--json` is the complete diff report. `diff` also reports a note when a shared record type's generated new-to-old converter
is guaranteed to fail because the older version requires a member the newer version removed; the
notes do not change the change counts or `--check` result. To make any command's diagnostics
machine-readable, add `--format json` or `--format sarif`; the catalogue is in the
[diagnostics specification](https://github.com/asamarts/witforge/blob/main/docs/design/diagnostics.md).

For CI, [`asamarts/witforge`](https://github.com/asamarts/witforge) publishes an Action that
installs the matching release and can run the same checks; see
the [Action specification](https://github.com/asamarts/witforge/blob/main/docs/design/action.md).

## Non-Rust consumers

Use the protobuf output with the normal generator for the target language:

- Go: `protoc-gen-go`;
- TypeScript: `ts-proto` or another protobuf generator;
- Python: `protoc`, `grpcio-tools`, or another protobuf generator;
- Java, Kotlin, C#, C++, Swift, and others: their standard protobuf generators.

The generated language type is only the protobuf projection. Proto3 cannot express every WIT
decode rule: narrow integers need range checks, required `optional` and message fields need
presence checks, enums are closed, and variant/result `oneof` values must select a known case.
Implement those rules in the non-Rust adapter and test invalid as well as valid values. The
[Non-Rust implementation guide](guide/non-rust.md) specifies the complete profile.

witforge intentionally does not duplicate `wit-bindgen`'s runtime WIT bindings or own transport
framing. The stable protobuf numbers and JSON Schema are cross-language contract artifacts, not a
complete application protocol.

## Supported WIT subset

The current subset is deliberately pure and synchronous:

- package versions are `<major>.<minor>.0`;
- types and functions live in named interfaces;
- records, variants, enums, flags, aliases, `use` aliases, lists, options, and results are
  supported;
- resources, maps, async, futures, streams, tuples, chars, fixed-length lists, multi-package
  workspaces, and world-defined types are currently refused.
- named types and interfaces from packages under a version directory's standard `deps/` layout
  are supported as package-qualified references; worlds may import and export dependency
  interfaces, and glue covers dependency types and functions. See the
  [dependency specification](https://github.com/asamarts/witforge/blob/main/docs/design/dependencies.md).

Run `witforge check` for the exact diagnostic. The
[design overview](https://github.com/asamarts/witforge/blob/main/docs/design/overview.md) has the
full contract model and the reasons for each refusal.

## Dependencies and Serde

A version directory may use standard `deps/` packages. Put the dependency under
`wit/v0.1/deps/`, declare its package, and import its types or interfaces normally:

```wit
package example:app@0.1.0;

interface api {
    use example:shared/types@0.1.0.{item};
    get: func() -> item;
}

world plugin {
    import example:shared/types@0.1.0;
}
```

`witforge rust`, `proto`, `schema`, and `glue` generate the dependency bindings alongside the
local output. See the
[dependency specification](https://github.com/asamarts/witforge/blob/main/docs/design/dependencies.md)
for the generated files and dependency glue layout.

Serde is opt-in per project:

```toml
[project.demo.rust]
out = "generated/rust"
serde = true
```

The default generated output is unchanged; enabling Serde requires the consumer's `serde` crate
with the `derive` feature. `--serde` and `--no-serde` override the configuration.

## JSON projections

Plain Rust Serde, Draft 7 JSON Schema, and proto3-JSON project the same WIT values differently:

- generated Rust Serde uses Rust identifiers, so `start-byte` becomes `start_byte`;
- JSON Schema preserves the WIT name `start-byte`, requires every record property, and permits
  `null` for `option<T>`;
- proto3-JSON uses lowerCamelCase (`startByte`), decimal strings for 64-bit integers, and normally
  omits an absent optional value;
- none of the projections supplies application defaults such as "missing severity means warning."

Choose one authoritative JSON profile. Keep an explicit application adapter when an existing
protocol differs, and do not validate one projection with another projection's schema. See
[JSON projections](guide/json-projections.md) for the full comparison.

## Examples

- [`examples/single`](https://github.com/asamarts/witforge/tree/main/examples/single) — one
  version, all four generators, config-driven.
- [`examples/evolved`](https://github.com/asamarts/witforge/tree/main/examples/evolved) — released
  v0.1 plus draft v0.2, lock continuity and generated conversions.

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
