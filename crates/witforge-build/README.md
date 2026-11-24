# witforge-build

Build-script (`build.rs`) helper for compiling [witforge](https://github.com/asamarts/witforge)-generated
`.proto` schemas with pure-Rust `protox`, `prost-build`, and optional `pbjson-build` (`pbjson` feature).

## Usage

Add `witforge-build` under `[build-dependencies]` (first published on crates.io starting in
`0.8.0`; enable `features = ["pbjson"]` if you also need proto3-JSON
`serde::Serialize` / `serde::Deserialize` implementations):

```toml
[dependencies]
prost = "0.14"

[build-dependencies]
witforge-build = "0.8.0"
```

In `build.rs`:

```rust,no_run
fn main() -> Result<(), Box<dyn std::error::Error>> {
    witforge_build::compile_protos("src/generated/proto")?;
    Ok(())
}
```

In `src/lib.rs` (the same layout works for both standalone contracts and contracts with `deps/`):

```rust,ignore
#[path = "generated/rust/mod.rs"]
pub mod plain;

#[allow(clippy::all, missing_docs)]
pub mod proto {
    include!(concat!(env!("OUT_DIR"), "/prost_include.rs"));
}

#[path = "generated/glue/mod.rs"]
pub mod glue;
```
