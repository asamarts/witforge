# Fuzz targets

The fuzz workspace is separate from the main Cargo workspace because `cargo-fuzz` runs under a
nightly toolchain with sanitizers.

```sh
cargo install cargo-fuzz --version 0.13.2
rustup toolchain install nightly-2026-09-11 --component rust-src
cargo +nightly-2026-09-11 fuzz run wit_package \
  --target x86_64-unknown-linux-gnu -- -runs=100
cargo +nightly-2026-09-11 fuzz run lockfile \
  --target x86_64-unknown-linux-gnu -- -runs=100
```

On Linux, use the GNU target: cargo-fuzz's default `x86_64-unknown-linux-musl` target is
incompatible with the address sanitizer unless static linking is disabled.

- `wit_package` writes arbitrary UTF-8 into a temporary `v0.1/contract.wit`, runs `load` and the
  subset checker, then exercises the model, lockfile reconciliation, and all four emitters.
- `lockfile` parses arbitrary UTF-8 lockfile text and re-renders anything that parses.

The harness is not a blocking stable-CI job. Run it manually before a release, after a parser
or emitter change, or when investigating a crash. A crash should become a committed regression
fixture rather than being left only in `fuzz/artifacts/`.
