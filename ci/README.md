# CI

CI runs exclusively on GitHub-hosted runners: Ubuntu for lint, docs, packaging, security, plan,
and generation, and the full Linux/macOS/Windows matrix for tests and published-Action smoke.
A self-hosted runner must never serve this public repository, where pull requests from forks could
execute code on it; the pre-0.1.0 self-hosted runner setup (`ci/Containerfile` and `ci/runner/`)
was retired when the repository became public and removed.

## Concurrency, toolchains, and schedule

- **Per-commit `main` concurrency:** pushes to `main` are grouped by `ci-<commit-sha>` with
  `cancel-in-progress: false`, so GitHub's single-pending-run concurrency limit can never cancel a
  queued `main` commit before it gets a `ci ok` result (which `release.yml` requires on the exact
  tag commit). Pull requests keep one group per ref with `cancel-in-progress: true`, and scheduled
  runs use `ci-schedule`. (The five retried CI runs on 2026-11-18 — runs `44371903590`,
  `44374904070`, `44380880018`, `44383409557`, and `44397862664` — were cancelled at ~15 minutes
  because `macos-15` runner queue delays hit their 10–15 minute job timeouts, not concurrency
  cancellation; `test` and `published-action` now allow 25 and 15 minutes respectively.)
- **Pinned lint/gate toolchain:** `ci.yml` pins `CI_RUST_TOOLCHAIN` (`1.100.0`) so a newly shipped
  Clippy or rustfmt lint arrives via a reviewed toolchain bump rather than breaking `main` mid-week.
  No top-level `rust-toolchain.toml` is committed, so contributors build with whatever Rust
  toolchain they already have installed (>= MSRV 1.88), and the `msrv` job stays pinned to `1.88`.
- **Weekly schedule:** every Monday at 06:00 UTC (and on `workflow_dispatch` with
  `scheduled_checks: true`), `ci.yml` runs on `main` including `security` (`cargo-audit` +
  `cargo-deny`), `floating-stable` (builds, formats, lints, docs, and tests on floating current
  `stable` to give early notice of upcoming toolchain lints), and `fuzz-smoke` (bounded `cargo-fuzz`
  runs under pinned nightly, uploading `fuzz/artifacts/` on failure). GitHub automatically disables
  scheduled workflows after 60 days without repository activity; failures alert the repository
  maintainers subscribed to GitHub Actions workflow failure notifications.

## Gates

The workflow in `.github/workflows/ci.yml` runs the normal Rust jobs plus:

- **tracked whitespace**: the lint job runs `git diff --check` from Git's empty tree to `HEAD`, so
  trailing whitespace and blank lines at end-of-file are rejected even when they entered in an
  earlier commit rather than the current diff.
- **workflows and scripts**: pinned, checksum-verified actionlint and shellcheck (the runner
  image's shellcheck changes version, and with it findings and codes) over both workflows, with
  shellcheck's masked-return check enabled for every `run:` script (a `$(...)` whose failure
  would not fail its step, as in `echo "x=$(cmd)"`), and shellcheck over `ci/`. It also runs
  `ci/test-release-asset.sh` for release archive/checksum generation,
  `ci/test-release-version.sh` for current-version reference checking, and `node --test
  action/test.js` for the Action's inputs, asset selection, and command plan. The reference
  check requires documentation to name the workspace version; `ci.yml` itself may name that
  version or the release before it, so that the commit preparing a release still runs a
  published Action and binary. Once the version's tag is on an earlier commit, the release is
  published and `ci.yml` must name it; the check sees that only in a checkout with its tags
  (the `generated` job fetches them). `cargo test` runs the reference-check cases and the
  Action's unit tests too (`crates/witforge-cli/tests/tooling.rs`), which gives both suites a
  test name that a plan task can cite.
- **published Action**: invokes the pinned remote release ref on Linux x86_64, macOS arm64,
  macOS Intel, and Windows x86_64. Each job downloads its platform archive, verifies its checksum,
  extracts it, adds it to `PATH`, and runs a real contract check. The workflows job separately
  executes `uses: ./` so Action changes are covered before a release exists.
- **generated**: builds the CLI and runs `ci/check-generated.sh`, which generates all four
  emitters' output for the versions fixture into a temporary tree and runs `--check` on each
  against it, so generation must succeed and reproduce itself byte for byte (the golden tests
  pin the output's content). Then `ci/check-release-freeze.sh` on the versions fixture: it reads
  the full history of `released`, merges included, so an entry once committed can never be
  removed and a released directory can never differ from any commit that listed it, whatever
  the push or pull request. `ci/test-release-freeze.sh` tests that script; the test jobs run it
  on Linux and, with the stock bash 3.2, on macOS. `ci/check-release.sh` also runs here without
  a tag, so README/docs/Action current-version references and the changelog section are checked
  on every main/PR run, not only during a release.
- **documented output and links**: the CLI distribution and docs tests compare the README and
  generated-output guide excerpts with committed examples, execute the README, quickstart,
  evolution, and wiring-crate walkthroughs (compiling and round-tripping the wiring crate), verify
  `docs/guide/reference.md` against `Cli::command()`, and verify every documentation cross-link
  and heading anchor for both GitHub (relative `.md` paths) and the built GitHub Pages site
  (rewritten `.html` paths, `docs/_config.yml` exclusions, and absolute Pages/GitHub URLs).
- **security**: installs pinned `cargo-audit` and `cargo-deny`, then runs
  `ci/check-security.sh`: RustSec advisories, the `ci/deny.toml` advisory/ban/license/source
  policy, and the public [`SECURITY.md`](../SECURITY.md) policy.
- **benchmark**: runs the small/large contract budget in release mode; property tests run in
  the normal test matrix and in the MSRV job.
- **package**: `cargo publish --workspace --dry-run`, which packages and verifies both crates,
  and `ci/test-publish.sh`, which checks that a repeated `ci/publish.sh --dry-run` verifies the
  CLI against the current library rather than a copy cargo cached for an earlier run.
- **public API compatibility**: installs cargo-semver-checks 0.51.0 and compares the library
  with the newest release on crates.io that is not newer than the workspace version, using
  `--release-type minor`. The baseline is not named in the workflow, so it advances with each
  release. The override deliberately denies breaking changes for the 1.0 candidate instead of
  treating the major bump as permission.
- **buf** (workflow dispatch only): optional `buf breaking` through `ci/check-buf-history.sh`.
  The old schema is generated from each release tag by that tag's published witforge binary;
  the candidate binary generates only the new schema. Every fixture WIT root is checked against
  every release tag at which it exists, so a new release or fixture joins the comparison without
  an edit. Each release is installed with `cargo install`, so a release that cannot be
  installed (yanked, or tagged and never published) fails the run until it is listed, with its
  reason, in `ci/buf-history-skip`. The manual `ci.yml` job caches compiled historical baseline
  binaries between runs (`actions/cache` keyed by runner OS and the release-tag list); `release.yml`
  deliberately does **not** cache them so a poisoned cache cannot pass the release gate. The job
  installs pinned buf 1.74 and fails if any tool cannot run.

- **ci ok**: passes only when every job above passed (the optional/scheduled jobs `buf`,
  `floating-stable`, and `fuzz-smoke` may be skipped on normal pushes/PRs). It is the one check
  the repository's `main` ruleset requires before a pull request merges, so adding or renaming a
  job never leaves the rule waiting on a check that no longer runs.

## Fuzzing

[`fuzz/`](../fuzz/) is a separate `cargo-fuzz` workspace because fuzzing runs under a nightly
toolchain with sanitizers. The campaign is not part of the stable CI gate; run it manually before
a release, after a parser or emitter change, or when investigating a crash. Stable CI does run
`cargo fmt --manifest-path fuzz/Cargo.toml --check` and
`cargo check --manifest-path fuzz/Cargo.toml --locked`, and the security job audits
`fuzz/Cargo.lock` and runs the dependency policy against the fuzz workspace.

```sh
cargo install cargo-fuzz --version 0.13.2
rustup toolchain install nightly-2026-09-11 --component rust-src
cargo +nightly-2026-09-11 fuzz run wit-package \
  --target x86_64-unknown-linux-gnu -- -runs=100
cargo +nightly-2026-09-11 fuzz run lockfile \
  --target x86_64-unknown-linux-gnu -- -runs=100
```

Use the GNU target on Linux: cargo-fuzz's default musl target is incompatible with the address
sanitizer unless static linking is disabled.

`wit-package` writes the input as a temporary `v0.1/contract.wit`, runs `load`, the subset
checker, model construction, lockfile reconciliation, and all four emitters. `lockfile` parses
arbitrary lockfile text and re-renders anything that parses. A discovered crash should become a
committed regression fixture under the relevant test suite, not only an artifact in
`fuzz/artifacts/`.

## Local test scratch

Every `ci/*.sh` script takes its scratch directory from `TMPDIR` and defaults to `$HOME/.tmp` when
`TMPDIR` is unset (never `/tmp`, which may be a small tmpfs on a shared host). Rust tests use
`std::env::temp_dir()`, which also follows `TMPDIR`:

```sh
mkdir -p "$HOME/.tmp"
TMPDIR="$HOME/.tmp" cargo test --workspace --locked
```

The Action's own temporary directory is inside the disposable GitHub runner, not a shared host.

## Repository settings

- **Rulesets.** `main` cannot be deleted or force-pushed, and a pull request into it needs
  `ci ok`. Release tags (`v*.*.*`) cannot be moved or deleted. Repository admins can bypass
  both, so a broken check never blocks a fix, but pull requests are the normal path.
- **Dependabot** (`.github/dependabot.yml`) opens one grouped pull request per week for cargo
  and one for GitHub Actions; CI, including the MSRV job, decides whether it can merge.
  Dependabot alerts are on.
- **Secret scanning** and push protection are on.

The aplan plan (`aplan check`, `aplan verify`, `aplan export-agents-md --check AGENTS.md`, and
`aplan check-docs`) runs in the **plan** job on a GitHub-hosted Ubuntu runner. The job installs
the pinned published release, **aplan 0.6.0**, from crates.io with `cargo install --locked`; no
aplan binary is committed to this repository. The job is part of `ci ok`.

## Releases

`.github/workflows/release.yml` runs when a `vX.Y.Z` tag is pushed:

- **verify**: `ci/check-release.sh` requires one version for both crates, `witforge-cli`'s
  dependency on `witforge` to require that version, a dated `CHANGELOG.md` section with entries
  for it, and a tag that matches the version, points at the checked-out commit, and is on `main`.
  For a tag push, it also requires a successful `ci ok` job whose head SHA is that exact commit.
  Then it runs the tests, the semver check against the previous release, uncached
  historical-generator protobuf compatibility against every earlier release,
  `ci/publish.sh --dry-run`, and records the deterministic `.crate` SHA-256 digests as job outputs.
- **binaries**: builds every platform archive and `.sha256` checksum, attaches a GitHub build
  provenance attestation (`actions/attest-build-provenance`; `id-token: write` and
  `attestations: write` exist ONLY in this job), and uploads the archive and checksum as a
  workflow artifact.
- **smoke-artifacts**: downloads each platform's workflow artifact onto its native runner, verifies
  both the `.sha256` checksum and the build provenance attestation (`gh attestation verify`), and
  exercises `--version`, `check`, `diff`, and `generate --check` on `examples/evolved` **before**
  anything is published.
- **publish** (environment `release`, which only `v*` tags may use): runs only after
  `smoke-artifacts` passes. Compares deterministic `.crate` digests against `verify`'s outputs,
  mints a short-lived crates.io token through trusted publishing only if a crate is not yet on
  crates.io, and uploads missing crates with `ci/publish.sh --no-verify` (running zero dependency
  build scripts while holding the publish token).
- **github-release**: downloads all built workflow artifacts, creates the GitHub release as a
  **draft** (or reuses an existing release on re-run), uploads each asset without `--clobber`
  (verifying SHA-256 digests of any already-uploaded asset and failing on mismatch), and publishes
  the draft only once every asset is attached.
- **tagged Action smoke**: after the GitHub release is published, invokes the checked-out tag's
  Action on every native platform runner to verify remote asset download, checksum/attestation
  verification, `check`, `diff`, and all four configured generators with `--check`. Main CI's
  pinned remote-ref matrix independently proves published tag resolution.

A manual run (`gh workflow run release.yml`, from a branch or a tag) runs only **verify**:
**publish** and **github-release** require a tag push (`github.event_name == 'push'`), not just
a tag ref, which a manual run from a tag has too. A re-run of a tag push keeps its event, so it
still publishes what is missing.

Each crate trusts this repository, `release.yml`, and the `release` environment on crates.io
(the crate's Settings, Trusted Publishing). crates.io cannot do that for a crate that does not
exist yet, so a new crate's first version is published by hand with a short-lived API token
in `CARGO_REGISTRY_TOKEN` (`ci/publish.sh`), after which its trusted publisher is added and the
token revoked; the release checklist has the steps.
