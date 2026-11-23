# 1.0 release criteria and deprecation runways

Status: criteria published 2026-11-01; updated 2026-11-20 after the second audit (`W10-098`); **1.0.0 has not been released**.

1.0 is a promise about the library API, generated output, and the conformance suite. Feature
completeness alone is not enough.

## Release criteria

- [ ] the public API inventory in [`public-api.md`](public-api.md) is reviewed and documented;
      `cargo doc` and `missing_docs` are clean; `cargo semver-checks check-release -p witforge
      --release-type minor` reports no breaking change against the newest published release,
      including for `WorldInterface` and `Model.dependencies`. Releases 0.4.2 to 0.5.0 each
      passed this check against 0.4.1, so comparing with the newest release keeps the surface
      reviewed at 0.4.1 covered. **One planned pre-1.0 exception:** the pre-1.0 minor release
      that lands `W10-095` (marking library model, lockfile, and diagnostic types
      `#[non_exhaustive]` and making unlisted public items private, together with `W10-096`'s
      structured check findings) breaks the library interface on purpose. Before and on the
      0.7.0 tag commit, `ci.yml` and `release.yml` still run every minor-compatibility lint and
      require its failures to equal the exact reviewed W10-095/W10-096 inventory. Once that tag
      is an ancestor of `HEAD`, ordinary minor compatibility is restored automatically.
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
- [ ] the checked-out Action and the pinned published Action pass on Linux x86_64, Linux arm64,
      macOS arm64, macOS Intel, and Windows x86_64; each platform installs and verifies its own
      release archive (checksum + build provenance attestation)
- [ ] GitHub Private Vulnerability Reporting is enabled on `asamarts/witforge` (verified via
      `gh api repos/asamarts/witforge/private-vulnerability-reporting`) and `SECURITY.md` states
      the threat model and archive verification steps
- [ ] the alint consumer pin and its CI are green against the 1.0 candidate
- [ ] independent external consumer validation is recorded (`W10-001`; unchanged by the second
      audit)
- [ ] `CHANGELOG.md`, README status, workspace versions, and the release checklist agree
- [ ] with `BUF_REQUIRED=1`, `ci/check-buf-history.sh` compares candidate output with every
      published generator, from `v0.1.0` on, for every fixture WIT root that existed at that
      release; JSON Schema dependency references resolve from the generated files
- [ ] aplan `check`, `verify`, `export-agents-md --check`, and `check-docs` pass
- [ ] the tag commit has a successful `ci ok` job from the `push` event before the release
      workflow publishes; platform binaries are built, attested with SLSA build provenance, and
      smoke-tested from workflow artifacts before crates.io or the GitHub release is published;
      the GitHub release is drafted until all assets are uploaded without overwriting existing
      assets; `publish` verifies `.crate` digests and runs with `--no-verify`; the release owner
      records that exact commit, CI run URL, and publish run URL in the changelog section

### Post-second-audit criteria

Added on 2026-11-19 (`W10-098`) after [the second audit](audit-2026-11-18-second.md). These apply
in addition to the criteria above and to independent external adoption (`W10-001`):

- [ ] **Every second-audit task in `M-p10` (`W10-038` to `W10-098`) is closed with evidence, or
      declined in writing here and dropped with `aplan drop` so the plan and this document agree.**
      Proved by `W10-002`'s dependency graph (through `W10-069` for the 0.6.0 blockers and direct
      blockers for `W10-071`, `W10-076`, and `W10-079` to `W10-098`). Already declined and
      dropped: `W10-044` (2026-11-18, owner decision: skip 0.5.1 and ship its fixes in 0.6.0 under
      `W10-069`).
- [ ] **The `prost` compile-and-round-trip matrix and compiled example output run in CI and are
      required by `ci ok`.** Proved by `W10-068` and the `needs` list of `ci ok` in
      `.github/workflows/ci.yml`.
- [ ] **One consumer has taken a contract through a minor evolution and regenerated it across two
      witforge releases.** This does not replace independent adoption (`W10-001`); `alint`'s proof
      crate can satisfy it by evolving its contract across a minor version and moving its pin
      across two witforge releases, proved by the two changes in its repository.
- [ ] **All four 1.0 surface-freezing decisions are implemented, not only decided:**
      major-version boundaries and version retirement (`W10-093`), `#[non_exhaustive]` library
      types and tightened public API table (`W10-095`), lossless JSON Schema result representation
      (`W10-070`), and unified proto/prost/glue identifier conversion (`W10-059`). Proved by
      closure of `W10-059`, `W10-070`, `W10-093`, and `W10-095`.

#### Post-1.0 ideas from the second audit (not required for 1.0)

Audit finding D7 recorded two ideas in cut `M-p11` rather than defects:

- `W11-038` (generated trait adapters between contract versions): **not required for 1.0**;
  remains a post-1.0 evaluation after `W10-093`.
- `W11-039` (protobuf file options for non-Rust generators): **not required for 1.0**; remains a
  post-1.0 evaluation after `W10-001` and `W11-034`.

### Exact compatibility commands

```sh
cargo semver-checks check-release -p witforge --release-type minor

BUF_REQUIRED=1 ci/check-buf-history.sh
```

Neither command names a baseline. cargo-semver-checks compares with the newest release on
crates.io that is not newer than the workspace version. `ci/check-buf-history.sh` runs
`ci/check-buf.sh` for every `crates/witforge/tests/fixtures/*/wit` root against every `vX.Y.Z`
tag except one that points at the commit being checked. Until 2026-11-18 both gates named
0.4.1 and so did not cover 0.4.2 to 0.5.0.

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
