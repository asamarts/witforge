# Quality and supply-chain gates

Status: implemented 2026-11-05 (`W8-004`).

witforge's correctness gates are layered so a golden-file test does not have to carry the whole
burden.

## Fuzz and property tests

[`crates/witforge/tests/properties.rs`](../../crates/witforge/tests/properties.rs) uses `proptest`
for two classes of input:

- arbitrary lockfile text must not panic; any text that parses must round-trip through
  `WireLock::parse` and `render`;
- generated valid models with random field, parameter, and result types must pass the Rust,
  protobuf, JSON Schema, and glue emitters without panicking.

The existing lockfile property suite also checks idempotent reconciliation, stable numbers, and
append-only reservations. The properties are intentionally input-level: they exercise parser and
emitter boundaries with shapes the hand-written golden fixtures may not cover.

## Benchmark budget

[`crates/witforge/tests/bench.rs`](../../crates/witforge/tests/bench.rs) measures a representative
small contract and a large contract through loading, modeling, lockfile reconciliation, and all
four emitters. The CI benchmark job runs it in release mode. The budgets are deliberately
catastrophic-regression ceilings rather than microbenchmark claims: 5 seconds for the small
contract and 15 seconds for the large one on a shared GitHub-hosted runner. The printed timings
make a regression visible in the job log.

## Dependency and advisory gates

[`ci/check-security.sh`](../../ci/check-security.sh) runs:

```sh
cargo audit
cargo deny check --config ci/deny.toml advisories bans licenses sources
```

The policy in [`ci/deny.toml`](../../ci/deny.toml) refuses known vulnerabilities, yanked crates,
wildcard dependencies, and unknown registries or Git sources. The license allow-list covers the
workspace's current dependency graph; a new dependency that adds a new license must update the
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
- the aplan plan gates and the release checklist.
