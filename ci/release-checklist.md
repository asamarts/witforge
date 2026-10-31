# 0.1.0 release checklist

Status: prepared 2026-10-29, updated 2026-10-31: the repository is public, hosted CI is green,
and the self-hosted runner is retired; publishing to crates.io (the release owner's token) is
next. This checklist is the path from the private 0.1.0 candidate to a public repository and a
published release.

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
- [x] `ci/check-release-freeze.sh crates/witforge/tests/fixtures/versions/wit <merge-base>`
      (the seven checks above: CI run 41240165506 on `main` at `dd0e219`, 2026-10-31, on
      GitHub-hosted runners, with the tests on Linux, macOS, and Windows)
- [x] `aplan check && aplan verify && aplan export-agents-md --check AGENTS.md && aplan check-docs`
      (local until a published aplan release lets CI run it; run at `dd0e219`, 2026-10-31)
- [ ] README status, `CHANGELOG.md` (with the publish date), and the workspace version say
      0.1.0. (README and version do; the changelog date is set when publishing.)
- [x] The release branch has no unintended generated-file or fixture drift (the generated-output
      job in the run above).

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
- [ ] `cargo publish -p witforge`, then `cargo publish -p witforge-cli` once the library is
      visible on crates.io (the CLI has a registry dependency on `witforge = "0.1.0"`).
      `cargo publish --workspace` does both in that order.
- [ ] Tag the published commit `v0.1.0` and create the GitHub release with the changelog entry.
- [ ] Verify the published crates install:
      `cargo install witforge-cli --version 0.1.0 --locked`.

## After publishing

- [ ] Record the release commit and CI run in the changelog or release notes.
- [ ] Keep the released fixtures and generated-output checks in CI.
- [ ] Move the next contract change to a new minor version directory with `--against`.
