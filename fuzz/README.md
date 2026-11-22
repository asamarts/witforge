# Fuzz targets

The fuzz workspace is separate from the main Cargo workspace because `cargo-fuzz` runs under a
pinned nightly toolchain with sanitizers.

```sh
cargo install cargo-fuzz --version 0.13.2
rustup toolchain install nightly-2026-09-11 --component rust-src
cargo +nightly-2026-09-11 fuzz run wit-package fuzz/seeds/wit-package \
  --target x86_64-unknown-linux-gnu -- -dict=fuzz/wit.dict -max_total_time=45
cargo +nightly-2026-09-11 fuzz run lockfile fuzz/seeds/lockfile \
  --target x86_64-unknown-linux-gnu -- -max_total_time=45
cargo +nightly-2026-09-11 fuzz run structured-contract fuzz/seeds/structured-contract \
  --target x86_64-unknown-linux-gnu -- -max_total_time=45
```

On Linux, use the GNU target: cargo-fuzz's default `x86_64-unknown-linux-musl` target is
incompatible with the address sanitizer unless static linking is disabled.

## Targets, seeds, and invariants

- `wit-package` (seeded by `fuzz/seeds/wit-package/` with `fuzz/wit.dict`): raw UTF-8 parser
  robustness target. Whenever an input parses and passes `subset::check_diagnostics`, it asserts
  the full compiler invariant suite below rather than discarding results.
- `structured-contract` (seeded by `fuzz/seeds/structured-contract/`): structure-aware contract
  generator producing valid WIT contracts inside the supported subset—including one-letter and
  upper-case identifiers, Rust/proto/WIT keywords, multiple `.wit` files, and `deps/` packages—and
  asserting every compiler invariant:
  1. a contract passing `subset::check_diagnostics` builds its model, extracts members, reconciles
     its lockfile, and succeeds across all four emitters (`rust`, `proto`, `schema`, `glue`);
  2. all four emitters are byte-for-byte deterministic across repeated runs;
  3. the rendered lockfile parses back, re-renders byte-identically, is canonical, and is
     idempotent when reconciled a second time;
  4. `diff::compare(v, v)` is empty;
  5. two identical adjacent minors convert cleanly (`convert.rs`).
- `lockfile` (seeded by `fuzz/seeds/lockfile/`): parses arbitrary UTF-8 lockfile text and asserts
  canonical round-trip and re-render identity.

The tracked seed directories live under `fuzz/seeds/`; the mutable working corpus (`fuzz/corpus/`)
stays ignored by Git. All scratch directories are created under `TMPDIR` (via
`std::env::temp_dir()`) and removed on drop.

The same structure-aware generator and invariants run in the stable test suite as
`structured_contracts_satisfy_fuzz_invariants` (`crates/witforge/tests/properties.rs`). The weekly
scheduled CI job (`fuzz-smoke` in `.github/workflows/ci.yml`) runs all three targets for a bounded
time and uploads `fuzz/artifacts/` if any crash occurs. A crash found by fuzzing should become a
committed regression test in the stable suite before it is fixed.
