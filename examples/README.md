# witforge examples

Two runnable examples with committed generated output:

- [`single/`](single/) is one contract version and exercises Rust, protobuf, JSON Schema, glue,
  `witforge.toml`, and all four generators' `--check` mode.
- [`evolved/`](evolved/) has released `v0.1` and draft `v0.2`; it demonstrates `lock --against`,
  continuity, release freezing, and generated old-to-new and new-to-old conversions.

Run from this repository with the local CLI:

```sh
cargo run -p witforge-cli -- check examples/single/wit/v0.1
cd examples/single
cargo run --manifest-path ../../Cargo.toml -p witforge-cli -- rust --config witforge.toml
cargo run --manifest-path ../../Cargo.toml -p witforge-cli -- proto --config witforge.toml
cargo run --manifest-path ../../Cargo.toml -p witforge-cli -- schema --config witforge.toml
cargo run --manifest-path ../../Cargo.toml -p witforge-cli -- glue --config witforge.toml
```

For the evolved example, check the draft against the released lockfile before generating:

```sh
cargo run -p witforge-cli -- check examples/evolved/wit/v0.2 \
  --against examples/evolved/wit/v0.1/wire.lock
cd examples/evolved
cargo run --manifest-path ../../Cargo.toml -p witforge-cli -- rust --config witforge.toml
```

Generated output is committed under each example's `generated/` directory. Run a generator
without `--check` to refresh it, review the diff, and rerun with `--check`. `v0.1` in the evolved
example is released, so do not edit its WIT, lockfile, or generated Rust module; add `v0.3` for
the next contract change.
