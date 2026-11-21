# Quality and supply-chain gates

Status: implemented 2026-11-05 (`W8-004`); expanded property coverage 2026-11-18 (`W10-022`);
fuzz workspace and CI integration 2026-11-18 (`W10-030`); `prost` compile matrix and example
compilation 2026-11-21 (`W10-068`).

witforge's correctness gates are layered so a golden-file test does not have to carry the whole
burden.

## Fuzz and property tests

[`crates/witforge/tests/properties.rs`](../../crates/witforge/tests/properties.rs) uses `proptest`
for these classes of input:

- arbitrary lockfile text must not panic; any text that parses must round-trip through
  `WireLock::parse` and `render`;
- generated models containing every type-definition kind, every type expression, a world, a
  function, named aliases, and named structural types must pass the Rust, protobuf, JSON Schema,
  and glue emitters without panicking;
- the two-version fixture must plan conversions and use `diff::compare_with_impacts` without
  panicking.

The existing lockfile property suite also checks idempotent reconciliation, stable numbers, and
append-only reservations. The properties are intentionally input-level: they exercise parser and
emitter boundaries with shapes the hand-written golden fixtures may not cover.

[`fuzz/`](../../fuzz/) is a separate `cargo-fuzz` workspace with `wit-package` and `lockfile`
targets. It runs under pinned `nightly-2026-09-11` with `rust-src` and the GNU target, and is not
a blocking stable-CI job. Stable CI still formats and checks the fuzz manifest, and the security
job audits the fuzz lockfile and runs the dependency policy against it. See
[`ci/README.md`](../../ci/README.md) for the reproducible invocation.

## Compile and round-trip matrix

[`crates/witforge/tests/support/mod.rs`](../../crates/witforge/tests/support/mod.rs) (re-exported
by [`crates/witforge-cli/tests/support/mod.rs`](../../crates/witforge-cli/tests/support/mod.rs))
is the single compile-and-round-trip harness used across `compile_matrix.rs`, `emit_glue.rs`,
`deps_glue.rs`, `deps_glue_full.rs`, and `e2e.rs`:

- **Offline, cached scratch builds under `TMPDIR`:** every scratch crate is created under
  `std::env::temp_dir()` with short Windows-safe directory names, copies the workspace's
  `Cargo.lock`, compiles `--offline` against the locked `prost = "0.14"` / `prost-build = "0.14"`
  (the supported `prost-build` release; `W10-059`), `protox = "0.10"`, and `pbjson = "0.9"` /
  `pbjson-build = "0.9"`, and shares a per-`rustc`-version build directory (`$TMPDIR/wf-t-<ver>`)
  so dependencies are compiled once per toolchain. Scratch crate directories are deleted on success
  and kept (with their path printed to stderr) on failure.
- **Batched modules and dual editions:** `HarnessBatch` bundles many contracts as modules inside
  one library crate (`wf-harness`) with a tiny runner binary, building each batch in both Rust
  Edition 2021 (`resolver = "2"`) and Rust Edition 2024 (`resolver = "3"`).
- **Quality gates on every generated module:** generated plain Rust and glue modules are checked
  with `rustfmt --edition <E> --style-edition <E> --check`, `cargo fmt --check`,
  `cargo clippy --all-targets -- -D warnings`, `#[deny(missing_docs)]`,
  `RUSTDOCFLAGS='-D warnings' cargo doc --lib --no-deps`, `cargo test --doc`, and binary execution
  of plain → prost → protobuf bytes → prost → plain (plus `pbjson` proto3 JSON where enabled)
  round-trips.

[`crates/witforge/tests/compile_matrix.rs`](../../crates/witforge/tests/compile_matrix.rs) runs
four suites through the harness:

1. **Name matrix:** one-letter words, upper-case words, digits, every Rust 2015–2024 keyword
   (including `self`/`super`/`crate`/`Self`, `gen`, `extern`, and `union`), every proto3 keyword,
   and type-only same-name interfaces across package, dependency, interface, type, field, case,
   flag, function, parameter, and world positions—plus explicit `Expected::CheckRefuses` rows that
   assert colliding/reserved names are rejected by `witforge check` with their exact codes.
2. **Shape matrix:** every type kind (record, variant with/without payload, enum, flags, alias,
   named and inline `list`/`option`/all four `result` forms) in every position, three levels of
   nesting, both byte-list spellings (`list<u8>` and `type byte = u8; list<byte>`), unreferenced
   named structural types, two-hop dependency-declared types, and explicit `Expected::CheckRefuses`
   rows.
3. **Fixed-seed property sample:** samples 24 contracts from the shared `propgen` generator
   (`crates/witforge/tests/support/propgen.rs`) with a fixed 32-byte seed and compiles every
   passing contract (`>= 8` required) through all quality gates in Editions 2021 and 2024.
4. **Committed examples:** checks `examples/single` and `examples/evolved` for drift and compiles
   their committed `generated/{rust,proto,glue}` outputs unmodified in Editions 2021 and 2024,
   running protobuf binary, `pbjson` JSON, `worlds`, and `convert` (`v0_1` ↔ `v0_2`) round-trips.

The full `compile_matrix` binary runs in both the cross-OS `test` matrix (Linux, macOS, Windows)
and the MSRV 1.88 job under `cargo test --workspace --locked`, and `ci/check-generated.sh`
explicitly executes `examples_generated_code_compiles_and_roundtrips` alongside `--check`.
Measured locally on a warm shared target directory: ~15s (`name_matrix`), ~25s (`shape_matrix`),
~25s (`property_sample`), and ~27s (`examples_generated_code`) running concurrently (~30–45s
wall-clock); CI budget is 180s on a shared GitHub-hosted runner.

## Benchmark budget

[`crates/witforge/tests/bench.rs`](../../crates/witforge/tests/bench.rs) measures representative
small and large single-version contracts through loading, modeling, lockfile reconciliation, and
all four emitters; the generated contracts include records, enums, variants, flags, and functions.
It also measures `diff::compare_with_impacts` and Rust conversion planning across a generated
two-version contract. The CI benchmark job runs both tests in release mode. The budgets are
deliberately catastrophic-regression ceilings rather than microbenchmark claims: 5 seconds for
the small contract, 15 seconds for the large one, and 5 seconds each for the two-version diff and
conversion plan on a shared GitHub-hosted runner. The printed timings make a regression visible
in the job log.

## Dependency and advisory gates

[`ci/check-security.sh`](../../ci/check-security.sh) runs:

```sh
cargo audit
cargo deny check --config ci/deny.toml advisories bans licenses sources
cargo audit --file fuzz/Cargo.lock
( cd fuzz && cargo deny check --config ../ci/deny.toml advisories bans licenses sources )
```

The policy in [`ci/deny.toml`](../../ci/deny.toml) refuses known vulnerabilities, yanked crates,
wildcard dependencies, and unknown registries or Git sources. The license allow-list covers both
the workspace and fuzz dependencies; a new dependency that adds a new license must update the
list deliberately. The CI security job installs pinned tool versions before running the script.

## Security policy

[`SECURITY.md`](../../SECURITY.md) is the public reporting policy. Reports go through GitHub
private vulnerability reporting, not public issues. Release archives carry SHA-256 checksums,
and the Action verifies them before extraction.

## Existing gates

These layers supplement, rather than replace:

- `cargo test --workspace --locked`, the MSRV 1.88 job, Clippy, rustdoc, and rustfmt;
- golden emitter, lockfile, conformance, and end-to-end tests;
- generated-output `--check` and release-freeze checks;
- fuzz-manifest formatting/checking and fuzz-lockfile advisory/license/source scanning;
- the aplan plan gates and the release checklist.
