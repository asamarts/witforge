# Release checklist

How witforge releases are cut. Every release after the first follows
[Later releases](#later-releases). The sections before it are the record of 0.1.0 (published
2026-10-31 from `546cec1`), which also made the repository public and published both crates by
hand, because crates.io cannot trust a repository for a crate that does not exist yet.

## Before publishing

- [x] alint's plugin contract consumes the candidate and its CI is green against the generated
      plain Rust, proto, and glue: alint PR #409 merged on 2026-10-31 (alint `6e437c61e`), and
      its `alint-witforge-consumer` crate compiles and round-trips under the full alint CI
      matrix.
- [x] The history is clean for a public repository: a committed binary of a private tool and a
      personal account name were removed from every commit before the repository went public
      (the pre-rewrite history is kept privately), and the commits cited as task evidence were
      remapped.
- [x] `cargo fmt --check`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `cargo test --workspace --locked`
- [x] `RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps`
- [x] `cargo +1.88 test --workspace --locked`
- [x] `ci/check-generated.sh`
- [x] `ci/check-release-freeze.sh crates/witforge/tests/fixtures/versions/wit`
      (the seven checks above: CI run 41255894743 on `main` at `546cec1`, the published commit,
      2026-10-31, on GitHub-hosted runners, with the tests on Linux, macOS, and Windows)
- [x] `aplan check && aplan verify && aplan export-agents-md --check AGENTS.md && aplan check-docs`
      (local until a published aplan release lets CI run it; run at `546cec1`, 2026-10-31)
- [x] README status, `CHANGELOG.md` (with the publish date), and the workspace version say
      0.1.0.
- [x] Generation succeeds and reproduces itself for the versions fixture (the generated-output
      job in the run above), and the golden tests pass.

## Make the repository public

The self-hosted runner MUST NOT serve a public repository. The workflow selects GitHub-hosted
runners when `github.event.repository.private` is false, but the runner transition is still an
explicit release-owner action.

- [x] Flip the GitHub repository visibility to public (2026-10-31).
- [x] Make sure GitHub Actions is enabled for the repository
      (`gh api repos/asamarts/witforge/actions/permissions`).
- [x] Run CI on `main` (`gh workflow run ci.yml`, or a push) and require every job green on
      GitHub-hosted runners, including the macOS and Windows test jobs, before publishing:
      run 41240165506, dispatched with `buf_breaking`, all seven jobs green.
- [x] Run `ci/runner/teardown.sh --purge` from the runner host, with `WITFORGE_REPO` set to the
      repository the runner is registered on (it refuses to change anything otherwise), then
      verify that repository's `actions/runners` list no longer has `witforge-runner`. Done
      against the private repository that holds the pre-rewrite history; no runner is
      registered on either repository.
- [x] Remove the `ci/runner/` installation from the host if it is no longer managed (the
      service, its unit, and the image are gone; `ci/runner/` stays in the tree for reference).
- [x] Do not leave stored runner credentials or volumes behind (the volumes held them and are
      deleted).

## Publish

- [x] Set `publish = true` in `crates/witforge/Cargo.toml` and
      `crates/witforge-cli/Cargo.toml`.
- [x] Verify `cargo package -p witforge` and
      `cargo publish --dry-run -p witforge --allow-dirty`.
- [x] `cargo publish --workspace --dry-run`: packages and verifies both crates, the CLI against
      the library through a temporary registry (at `dd0e219`, 2026-10-31).
- [x] Set the `CHANGELOG.md` date to the publish date (2026-10-31; change it if the publish
      slips); `ci/check-release.sh` passes.
- [x] `ci/publish.sh --dry-run` from the release checkout verifies both crates. It clears the
      copies of the library cargo unpacked and built for an earlier run, which a local dry run
      would otherwise verify the CLI against (`ci/test-publish.sh`).
- [x] Publish 0.1.0 by hand (2026-10-31, from `546cec1`; the published `.crate` files match
      that commit, and docs.rs built the library). crates.io cannot trust a repository for a crate that does not
      exist yet, so a new crate's first version needs an API token: create one for the
      publishing account with only the `publish-new` scope, the crate pattern `witforge*`, and
      the shortest expiry. From a clean checkout of the release commit on `main`, run
      `read -rs CARGO_REGISTRY_TOKEN && export CARGO_REGISTRY_TOKEN && ci/publish.sh`, which
      leaves `~/.cargo/credentials.toml` and the shell history alone. It publishes `witforge`,
      then `witforge-cli` once the library is on the index; a repeat skips what is published.
- [x] On crates.io, add a trusted publisher to each crate (done by the owner on 2026-10-31;
      crates.io shows the configuration only to the crate's owners, so the first release
      through `release.yml`, 0.1.1, was its first test, and it passed: release run
      41274576724 published both crates) (the crate's Settings, Trusted
      Publishing: GitHub, repository `asamarts/witforge`, workflow `release.yml`, environment
      `release`), and optionally allow only trusted publishing for the crate. Then revoke the
      token on crates.io and `unset CARGO_REGISTRY_TOKEN`.
- [x] Tag the published commit `v0.1.0` and push the tag. `release.yml` verifies the release,
      finds nothing left to publish, and creates the GitHub release from the changelog entry
      (release run 41261961612; its publish steps were skipped).
- [x] Verify the published crates install:
      `cargo install witforge-cli --version 0.1.0 --locked` (`witforge 0.1.0`, and alint's
      consumer check and its controls pass with that binary).

## Later releases

- [ ] Bump `version` under `[workspace.package]` in `Cargo.toml` and `witforge-cli`'s
      requirement on `witforge` in `crates/witforge-cli/Cargo.toml` to the new version (the CLI
      uses the library's API as of that version; `ci/check-release.sh` refuses a mismatch), and
      refresh `Cargo.lock` with `cargo update -w`, since CI builds with `--locked`.
- [ ] Turn `## [Unreleased]` in `CHANGELOG.md` into `## [X.Y.Z] - YYYY-MM-DD` (the tag date) and
      update the README's status line. Say in the section's intro whether generated output
      changes for a contract the previous release accepts (`git diff vPREV -- crates/*/tests`
      shows any changed golden output); consumers such as alint pin a release and regenerate
      when they move the pin.
- [ ] Run the gates of [Before publishing](#before-publishing), then `ci/check-release.sh` and
      `ci/publish.sh --dry-run`, and the aplan commands locally.
- [ ] Merge to `main` and wait for its CI run (`ci ok`) to pass.
- [ ] Tag that commit `vX.Y.Z` (annotated) and push the tag. `release.yml` verifies the release,
      publishes each crate whose version is not on crates.io yet, library first, through trusted
      publishing, and creates the GitHub release from the changelog section. A failed run is
      re-run and skips what is already published.
- [ ] Verify `cargo install witforge-cli --version X.Y.Z --locked` reports `witforge X.Y.Z`,
      the GitHub release exists, and docs.rs built the library.
- [ ] Record the release commit, its CI run, and the release run under the changelog section.

A new crate in the workspace needs its first version published by hand, as for 0.1.0 above. If
trusted publishing itself fails (for example a changed workflow or environment name that no
longer matches the crates.io configuration), fix the configuration rather than falling back to a
long-lived token; a one-off hand publish uses a token scoped to `publish-update` for the
affected crates, and re-running `release.yml` then only creates the GitHub release.

## After publishing

- [x] Record the release commit and CI run in the changelog or release notes (0.1.0: commit
      `546cec1`, CI run 41255894743, release run 41261961612; 0.1.1: commit `978c985`, CI run
      41274574941, release run 41274576724; 0.1.2: commit `3f89ec4`, CI run 41341872998,
      release run 41342094380).
- [x] Keep the released fixtures and generated-output checks in CI.
- [ ] Move the next contract change to a new minor version directory with `--against`.
