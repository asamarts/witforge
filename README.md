# witforge

[![CI](https://github.com/asamarts/witforge/actions/workflows/ci.yml/badge.svg)](https://github.com/asamarts/witforge/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/witforge-cli.svg)](https://crates.io/crates/witforge-cli)
[![docs](https://img.shields.io/badge/docs-guide-blue.svg)](https://asamarts.github.io/witforge/)
[![MSRV](https://img.shields.io/badge/MSRV-1.88-brightgreen.svg)](Cargo.toml)

**Compile one hand-written WIT contract into the runtime-independent artifacts its consumers
need.**

witforge checks a deliberately portable WIT subset, locks protobuf field and method numbers, and
generates stable Rust, protobuf, JSON Schema, glue, compatibility reports, and adapters between
contract versions.

Current release: **0.4.3**. The 1.0 release remains intentionally gated on successful independent
consumer validation.

## Outputs

| Output | Purpose |
|---|---|
| Plain Rust | Owned structs, enums, interface traits, world metadata, and optional Serde derives |
| Protobuf | Messages and method IDs with append-only numbers from `wire.lock` |
| JSON Schema | Draft 7 definitions for WIT-shaped values |
| Rust glue | Checked conversion between plain Rust and prost-generated protobuf types |
| Version adapters | Old-to-new and new-to-old conversion between adjacent contract versions |
| Diff and diagnostics | Human, JSON, and SARIF compatibility/change reports |

## When it fits

Use witforge when:

- hand-written WIT is the source of truth;
- the callable contract is pure, synchronous, and value-oriented;
- native Rust and out-of-process implementations share the same values;
- protobuf identity must remain stable across releases;
- generated output is committed and checked in CI;
- contract changes need explicit compatibility review.

Use [wit-bindgen](https://github.com/bytecodealliance/wit-bindgen) and a Component Model runtime
directly when you only need guest and host Component Model bindings.

witforge does not load components, generate protobuf services, own process framing, or define
callbacks, retries, capabilities, resource limits, discovery, or failure policy. Direct Go source
generation is not available yet; non-Rust consumers currently use the emitted protobuf schema and
their normal language toolchain.

## Install

```sh
cargo install witforge-cli --version 0.4.3 --locked
witforge --version
```

Prebuilt checksum files and archives are also published for Linux x86_64, macOS Intel and Apple
silicon, and Windows x86_64.

## Five-minute example

Create `wit/v0.1/contract.wit`:

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

Lock and validate the first version:

```sh
witforge lock wit/v0.1 --first
witforge check wit/v0.1
```

Generate the projections:

```sh
witforge rust wit --out generated/rust
witforge proto wit --out generated/proto
witforge schema wit --out generated/schema
witforge glue wit --out generated/glue \
  --plain-root crate::plain \
  --proto-root crate::proto
```

Every generator supports `--check`, which writes nothing and fails when committed output differs.

For configuration, dependencies, evolution, and complete command sequences, continue with the
[documentation](https://asamarts.github.io/witforge/) or the
[repository quickstart](docs/quickstart.md).

## Evolve a contract

Keep each contract version in its own directory. Start a new minor from the previous lockfile:

```sh
cp wit/v0.1/wire.lock wit/v0.2/wire.lock
witforge lock wit/v0.2 --against wit/v0.1/wire.lock
witforge check wit/v0.2 --against wit/v0.1/wire.lock
witforge diff wit/v0.1 wit/v0.2 --check
```

Released WIT, lockfiles, dependency packages, and generated per-version Rust modules are immutable.
Create a new version directory instead of editing a released one.

## CI

Pin the Action to the same release used locally:

```yaml
- uses: asamarts/witforge@v0.4.3
  with:
    check: wit/v0.2
    diff-old: wit/v0.1
    diff-new: wit/v0.2
    config: witforge.toml
    generators: rust,proto,schema,glue
```

The Action downloads the matching checksum-verified release binary and runs configured generators
with `--check`. See the [CI guide](https://asamarts.github.io/witforge/guide/ci.html).

## Important JSON boundary

Rust Serde and JSON Schema project the same WIT values, but they do not define one identical JSON
wire format. Rust fields are snake_case; JSON Schema preserves WIT spelling, normally kebab-case.
WIT `option<T>` permits `null` but does not make a schema property omittable.

Read [JSON Schema and Rust Serde](https://asamarts.github.io/witforge/guide/json-projections.html)
before applying both outputs to an existing JSON protocol.

## Documentation and examples

- [Documentation site](https://asamarts.github.io/witforge/)
- [Quickstart](docs/quickstart.md)
- [Single-version example](examples/single)
- [Evolved-contract example](examples/evolved)
- [Design overview](docs/design/overview.md)
- [Public API policy](docs/design/public-api.md)
- [Release criteria](docs/design/release-1.0.md)

The first consumer is the [alint](https://github.com/asamarts/alint) plugin contract. Task-level
planning and closure evidence live under [`aplan/`](aplan/README.md); [`ROADMAP.md`](ROADMAP.md)
is the narrative sequence.

## Security

Report vulnerabilities through
[GitHub private vulnerability reporting](https://github.com/asamarts/witforge/security/advisories/new),
not a public issue. See [`SECURITY.md`](SECURITY.md).

## License

Licensed under either [Apache-2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT), at your option.
