---
layout: default
title: Wiring a Rust crate and installing witforge
---

# Wiring a Rust crate and installing witforge

This guide takes an empty Rust crate to a compiling plain-Rust ↔ protobuf round trip, explains how
to install and verify prebuilt binaries, and covers the `--lock`, `--released`, and `released`
configuration options.

## Install and verify

### From crates.io

```sh
cargo install witforge-cli --version 0.8.0 --locked
witforge --version
```

### Prebuilt archives

Prebuilt archives are published on [GitHub Releases](https://github.com/asamarts/witforge/releases)
for native targets (each archive contains the executable beside `LICENSE-APACHE`,
`LICENSE-MIT`, and `README.md`, plus a sibling `.sha256` file and, from 0.7.0, a SLSA build
provenance attestation):

| Platform | Target | Notes |
|---|---|---|
| Linux x86_64 (static) | `x86_64-unknown-linux-musl` | Portable across glibc/Alpine (from 0.8.0) |
| Linux x86_64 (gnu) | `x86_64-unknown-linux-gnu` | glibc >= 2.39 (Ubuntu 24.04 runner) |
| Linux arm64 (static) | `aarch64-unknown-linux-musl` | Portable across glibc/Alpine (from 0.8.0) |
| Linux arm64 (gnu) | `aarch64-unknown-linux-gnu` | glibc >= 2.39, published from 0.7.0 |
| macOS Intel | `x86_64-apple-darwin` | — |
| macOS Apple silicon | `aarch64-apple-darwin` | — |
| Windows x86_64 | `x86_64-pc-windows-msvc` | — |

Windows `arm64` builds are **not** provided; install from crates.io with `cargo install
witforge-cli --locked` on that target. `cargo binstall witforge-cli` is also configured via
`[package.metadata.binstall]`.

Download and verify a prebuilt archive before extracting it:

```sh
VERSION=0.8.0
TARGET=x86_64-unknown-linux-musl
BASE="https://github.com/asamarts/witforge/releases/download/v${VERSION}"
curl -sSfLO "${BASE}/witforge-${VERSION}-${TARGET}.tar.gz"
curl -sSfLO "${BASE}/witforge-${VERSION}-${TARGET}.tar.gz.sha256"
sha256sum -c "witforge-${VERSION}-${TARGET}.tar.gz.sha256"
# Releases 0.7.0 and later also carry a GitHub build provenance attestation:
gh attestation verify "witforge-${VERSION}-${TARGET}.tar.gz" \
  --repo asamarts/witforge \
  --signer-workflow asamarts/witforge/.github/workflows/release.yml
tar -xzf "witforge-${VERSION}-${TARGET}.tar.gz"
./witforge --version
```

On macOS, use `shasum -a 256 -c` if `sha256sum` is unavailable.

## Layout: committed output vs. `build.rs`

Two layouts are possible:

1. **Commit witforge output under `src/generated/` (recommended):** `witforge generate` writes
   plain Rust, `.proto`, JSON Schema, and glue into committed directories, and `build.rs` only
   compiles the committed `.proto` files with `prost-build` (`0.14`) and `protox` (`0.10`, so no
   system `protoc` binary is required). CI runs `witforge generate --check` to catch drift, code
   review shows exact API changes in pull requests, and downstream crates build with plain `cargo`
   without installing `witforge`.
2. **Generate during `build.rs`:** invokes `witforge` into `OUT_DIR`. This avoids committing
   generated files, but `witforge --check` cannot guard committed per-version modules in CI and
   every contributor needs the matching `witforge` binary during `cargo build`.

The walkthrough below uses the committed layout. Generated glue targets **`prost` / `prost-build`
0.14** (and optionally `pbjson` / `pbjson-build` 0.9 for proto3 JSON).

## Empty crate to compiling round trip

Create the crate layout:

```text
my-contract-crate/
  Cargo.toml
  build.rs
  witforge.toml
  wit/
    v0.1/
      contract.wit
      wire.lock
  src/
    lib.rs
    main.rs
    generated/
      rust/
      proto/
      schema/
      glue/
```

```sh
mkdir -p my-contract-crate/wit/v0.1 my-contract-crate/src
```

Save `my-contract-crate/Cargo.toml`:

```toml
[package]
name = "my-contract-crate"
version = "0.1.0"
edition = "2024"

[dependencies]
prost = "0.14"

[build-dependencies]
prost-build = "0.14"
protox = "0.10"
```

Save `my-contract-crate/wit/v0.1/contract.wit`:

```wit
package example:greeter@0.1.0;

interface api {
    record greeting {
        text: string,
        repeat: option<u32>,
    }

    greet: func(name: string) -> greeting;
}

world hello {
    export api;
}
```

Save `my-contract-crate/witforge.toml`:

```toml
version = 1

[project.greeter]
root = "wit"

[project.greeter.rust]
out = "src/generated/rust"

[project.greeter.proto]
out = "src/generated/proto"

[project.greeter.schema]
out = "src/generated/schema"

[project.greeter.glue]
out = "src/generated/glue"
plain-root = "crate::plain"
proto-root = "crate::proto"
```

Lock the contract, validate it, and generate all four outputs:

```sh
witforge lock my-contract-crate/wit/v0.1 --first
witforge check my-contract-crate/wit
witforge generate --config my-contract-crate/witforge.toml
witforge generate --config my-contract-crate/witforge.toml --check
```

Save `my-contract-crate/build.rs`, which compiles the committed `.proto` schema using `protox`
(pure Rust, no external `protoc`) and `prost-build`:

```rust
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let proto_dir = PathBuf::from("src/generated/proto");
    println!("cargo:rerun-if-changed={}", proto_dir.display());
    let fds = protox::compile([proto_dir.join("v0.proto")], [&proto_dir])?;
    prost_build::Config::new().compile_fds(fds)?;
    Ok(())
}
```

Save `my-contract-crate/src/lib.rs`, exposing `plain`, `proto`, and `glue` at the exact module
paths configured by `plain-root = "crate::plain"` and `proto-root = "crate::proto"`:

```rust
#![deny(missing_docs)]

//! Contract crate wiring witforge's generated plain Rust, prost protobuf types, and glue.

/// Generated plain Rust contract types, traits, and worlds.
#[path = "generated/rust/mod.rs"]
pub mod plain;

/// Prost-generated protobuf messages compiled by `build.rs`.
pub mod proto {
    /// Wire schema for major version 0.
    #[allow(clippy::all, missing_docs)]
    pub mod v0 {
        include!(concat!(env!("OUT_DIR"), "/example.greeter.v0.rs"));
    }
}

/// Generated plain-Rust <-> prost protobuf glue.
#[path = "generated/glue/mod.rs"]
pub mod glue;
```

Save `my-contract-crate/src/main.rs` to round-trip a plain Rust value through generated glue and
protobuf binary encoding:

```rust
use my_contract_crate::{glue, plain, proto};
use prost::Message as _;

fn main() {
    let original = plain::v0_1::api::Greeting {
        text: "hello".to_string(),
        repeat: Some(3),
    };
    let wire = glue::v0::api::greeting::to_proto(&original);
    let encoded = wire.encode_to_vec();
    let decoded_wire = proto::v0::ApiGreeting::decode(encoded.as_slice()).expect("decode proto");
    let roundtripped = glue::v0::api::greeting::from_proto(decoded_wire).expect("glue from_proto");
    assert_eq!(roundtripped, original);
}
```

Run `cargo run` inside `my-contract-crate/` (the repository test suite builds and runs these exact
files verbatim):

```sh
cargo run --manifest-path my-contract-crate/Cargo.toml
```

## Module roots, dependencies, `rustfmt`, and lints

- **Module path forms:** `--plain-root` / `plain-root` and `--proto-root` / `proto-root` must be
  absolute Rust module paths: `crate`, `crate::...`, `::crate_name::...`, or an external crate
  name (`other::...`). Relative paths (`self::...`, `super::...`) and unprefixed generated module
  names (`v0`, `witforge_deps`, etc.) are refused with exit `2`.
- **Contracts with `deps/`:** When a contract uses standard `deps/` packages, `witforge proto`
  emits `<namespace>.<pkg>.v<major>.proto` files alongside `v<major>.proto`. Pass all `.proto`
  files in `src/generated/proto/` to `protox::compile(...)`, set
  `prost_build::Config::new().include_file("prost_include.rs")`, and include
  `concat!(env!("OUT_DIR"), "/prost_include.rs")` inside `pub mod proto { ... }` so prost emits
  the package-nested module tree (`crate::proto::<namespace>::<pkg>::v<major>`) that glue expects.
- **Formatting and lints:** Generated plain Rust and glue already match default `rustfmt`
  (Editions 2021 and 2024), pass default Clippy (`-D warnings`), and document every public item
  (`-D missing_docs`) without needing attributes. If your crate uses a non-default `rustfmt.toml`,
  add `#[rustfmt::skip]` on the `pub mod plain;` and `pub mod glue;` declarations.
- **`#![no_std]` + `alloc` crates:** Set `no-std = true` under `[project.<name>.rust]` and
  `[project.<name>.glue]` (or pass `--no-std` on `rust`, `glue`, or `generate`). Generated Rust,
  `convert.rs`, and glue emit `::alloc::string::String`, `::alloc::vec::Vec`,
  `::core::option::Option`, `::core::result::Result`, and `::core::error::Error` with no `::std::`
  references. In your crate, declare `#![no_std]` and `extern crate alloc;`, and depend on `prost`
  with `default-features = false, features = ["derive"]` (and `serde` with `default-features =
  false, features = ["alloc", "derive"]` if `serde = true`).

## Simplifying `build.rs` with `witforge-build`

Instead of wiring `protox`, `prost-build`, and manual `cargo:rerun-if-changed` directives by hand,
you can use the optional `witforge-build` helper crate (first published on crates.io starting in
`0.8.0`) in `[build-dependencies]`:

```toml
[build-dependencies]
witforge-build = "0.8.0"
```

```rust
// build.rs
fn main() -> Result<(), Box<dyn std::error::Error>> {
    witforge_build::compile_protos("src/generated/proto")?;
    Ok(())
}
```

Then include the generated `prost_include.rs` in `src/lib.rs`:

```rust
#[allow(clippy::all, missing_docs)]
pub mod proto {
    include!(concat!(env!("OUT_DIR"), "/prost_include.rs"));
}
```

`witforge-build` discovers every `.proto` file in deterministic order, emits per-file
`cargo:rerun-if-changed` directives, compiles with pure-Rust `protox` (no external `protoc`
needed), and generates a `prost_include.rs` layout matching `witforge glue` for both standalone
contracts (`crate::proto::v0::*`) and contracts with `deps/`
(`crate::proto::<namespace>::<pkg>::v<major>::*`). Enable `features = ["pbjson"]` and call
`witforge_build::Config::new().pbjson(true).compile_protos("src/generated/proto")?` to also emit
and include proto3-JSON `serde` implementations via `pbjson-build`.

## Released-directory options (`--lock`, `--released`, and `released`)

- **`--lock <PATH>`** (on `check` and `lock` for a single version directory): overrides the
  lockfile path instead of the default `<WIT_DIR>/wire.lock`. `witforge lock` still refuses any
  `--lock` path located inside a released directory.
- **`--released <PATH>`** (on `check`, `lock`, `rust`, `proto`, `schema`, `glue`, `generate`,
  `release`, `retire`, `inspect`, and `freeze`): overrides the released-version manifest path. The
  defaults are:
  - `<WIT_DIR>/../released` for single-version commands (`check <dir>`, `lock <dir>`,
    `release <dir>`, `retire <dir>`);
  - `<WIT_ROOT>/released` for whole-root `check`, `inspect` (refused on single version directories),
    generators, and `freeze`.
  A missing default manifest means no versions are released; an explicitly passed missing manifest
  exits `2`. For `lock`, an explicit `--released` manifest is checked **in addition to** the
  default manifests (never instead of them), so an explicit manifest cannot unlock a released
  directory.
- **`released = "..."` in `witforge.toml`:** sets the project's released-version manifest path
  relative to `witforge.toml` (overridden by CLI `--released`).
- **Version retirement (`<name> retired`):** marking a contiguous oldest prefix of released
  versions as `retired` in the manifest keeps their directories frozen on disk while removing them
  from generated outputs on the next run. See [Contract evolution](evolution.md) for major-version
  and retirement rules, and [CLI and configuration reference](reference.md) for every flag and key.
