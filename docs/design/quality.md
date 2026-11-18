# Quality and supply-chain gates

Status: implemented 2026-11-05 (`W8-004`); expanded property coverage 2026-11-18 (`W10-022`);
fuzz workspace and CI integration 2026-11-18 (`W10-030`).

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
