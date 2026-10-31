# 0.1.0 release checklist

Status: prepared 2026-10-29, updated 2026-10-31; execution is the release owner's step. This
checklist is the path from the private 0.1.0 candidate to a public repository and a published
release.

## Before publishing

- [x] alint's plugin contract consumes the candidate and its CI is green against the generated
      plain Rust, proto, and glue: alint PR #409 merged on 2026-10-31 (alint `6e437c61e`), and
      its `alint-witforge-consumer` crate compiles and round-trips under the full alint CI
      matrix.
- [x] The history is clean for a public repository: a committed binary of a private tool and a
      personal account name were removed from every commit before the repository went public
      (the pre-rewrite history is kept privately), and the commits cited as task evidence were
      remapped.
- [ ] `cargo fmt --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace --locked`
- [ ] `RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps`
- [ ] `cargo +1.88 test --workspace --locked`
- [ ] `ci/check-generated.sh`
- [ ] `ci/check-release-freeze.sh crates/witforge/tests/fixtures/versions/wit <merge-base>`
- [ ] `aplan check && aplan verify && aplan export-agents-md --check AGENTS.md && aplan check-docs`
      (local until a published aplan release lets CI run it)
- [ ] README status, `CHANGELOG.md` (with the publish date), and the workspace version say
      0.1.0.
- [ ] The release branch has no unintended generated-file or fixture drift.

## Make the repository public

The self-hosted runner MUST NOT serve a public repository. The workflow selects GitHub-hosted
runners when `github.event.repository.private` is false, but the runner transition is still an
explicit release-owner action.

- [ ] Flip the GitHub repository visibility to public.
- [ ] Make sure GitHub Actions is enabled for the repository
      (`gh api repos/asamarts/witforge/actions/permissions`).
- [ ] Run CI on `main` (`gh workflow run ci.yml`, or a push) and require every job green on
      GitHub-hosted runners, including the macOS and Windows test jobs, before publishing.
- [ ] Run `ci/runner/teardown.sh --purge` from the runner host, with `WITFORGE_REPO` set to the
      repository the runner is registered on (it refuses to change anything otherwise), then
      verify that repository's `actions/runners` list no longer has `witforge-runner`.
- [ ] Remove the `ci/runner/` installation from the host if it is no longer managed.
- [ ] Do not leave stored runner credentials or volumes behind.

## Publish

- [x] Set `publish = true` in `crates/witforge/Cargo.toml` and
      `crates/witforge-cli/Cargo.toml`.
- [x] Verify `cargo package -p witforge` and
      `cargo publish --dry-run -p witforge --allow-dirty`.
- [ ] `cargo publish --workspace --dry-run`: packages and verifies both crates, the CLI against
      the library through a temporary registry.
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
