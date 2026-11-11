# 1.0 release criteria and deprecation runways

Status: criteria published 2026-11-01; **1.0.0 has not been released**.

1.0 is a promise about the library API, generated output, and the conformance suite. Feature
completeness alone is not enough.

## Release criteria

- [ ] the public API inventory in [`public-api.md`](public-api.md) is reviewed and documented;
      `cargo doc` and `missing_docs` are clean; `cargo semver-checks check-release -p witforge
      --baseline-version 0.4.1 --release-type minor` reports no breaking change, including for
      `WorldInterface` and `Model.dependencies`
- [ ] the boundary error policy is published: `anyhow::Error` plus the stable `ContractError`
      classification, with no unclassified contract failure
- [ ] the conformance matrix and `tests/conformance.rs` cover every supported WIT construct and
      every emitter mapping row, including standard `deps/` types, foreign world interfaces and
      dependency glue, version conversion with foreign values, and opt-in Serde
- [ ] the release test matrix is green on Linux, macOS, and Windows, and on MSRV 1.88
- [ ] `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
      `cargo test --workspace --locked`, `RUSTDOCFLAGS='-D warnings' cargo doc --workspace
      --no-deps`, `ci/check-generated.sh`, `ci/check-release-freeze.sh
      crates/witforge/tests/fixtures/versions/wit`, and `ci/check-security.sh` pass; cargo-deny
      refuses yanked crates
- [ ] the checked-out Action and the pinned published Action pass on Linux x86_64, macOS arm64,
      macOS Intel, and Windows x86_64; each platform installs and verifies its own release archive
- [ ] the alint consumer pin and its CI are green against the 1.0 candidate
- [ ] `CHANGELOG.md`, README status, workspace versions, and the release checklist agree
- [ ] with `BUF_REQUIRED=1`, `ci/check-buf.sh` compares candidate output with each published
      generator `v0.1.2`, `v0.2.0`, `v0.3.0`, and `v0.4.1` for the versions fixture and with
      `v0.4.1` for every dependency fixture; JSON Schema dependency references resolve from the
      generated files
- [ ] aplan `check`, `verify`, `export-agents-md --check`, and `check-docs` pass
- [ ] the tag commit has a successful `ci ok` job from the `push` event before the release
      workflow publishes; the release owner records that exact commit, CI run URL, and publish
      run URL in the changelog section

### Exact compatibility commands

```sh
cargo semver-checks check-release -p witforge \
  --baseline-version 0.4.1 --release-type minor

for release in v0.1.2 v0.2.0 v0.3.0 v0.4.1; do
  BUF_REQUIRED=1 ci/check-buf.sh \
    crates/witforge/tests/fixtures/versions/wit "$release"
done
for fixture in deps deps-full deps-world deps-convert deps-unsupported; do
  BUF_REQUIRED=1 ci/check-buf.sh \
    "crates/witforge/tests/fixtures/${fixture}/wit" v0.4.1
done
```

`ci/check-buf.sh` installs or accepts the historical release binary for the old side; the
candidate binary generates only the new side. The tag release workflow runs these commands
again and queries the Actions API for a successful `ci ok` job whose `head_sha` is the tag
commit. A green run for another commit is not evidence.

## Deprecation runways

### Library

- A public item is deprecated with `#[deprecated]` and a replacement, then remains available for
  at least one minor release before removal in the next major.
- Removing or changing a public item, adding a field to a constructible public struct, or adding
  a variant to an exhaustively matched public enum is a major-version change.
- MSRV increases are a minor release and are documented in the changelog and release notes.

### Generated output and wire

- A released contract version directory, its lockfile, and its per-version Rust module are
  immutable; changes require a new version directory.
- Wire numbers, reserved names, and proto method ids are never removed or reused. A removed
  member stays reserved; removing a type or function keeps its method number reserved.
- Shared generated output (`mod.rs`, `convert.rs`, `worlds.rs`, per-major proto, glue) may gain
  additions when a new version is added, but released members remain protected by the lockfile
  and `--check`.
- CLI flags are deprecated one release before removal, except security fixes; a removed flag
  remains documented in the changelog.

### Contract versions

- A new major starts a fresh lockfile and a new protobuf package; a previous-major adapter is
  documented for a stated period.
- A new minor copies the previous lockfile and uses `lock --against`; declaration-order
  fingerprints and the append-only rules protect common members.
