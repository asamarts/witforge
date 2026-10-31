# 0.1.0 release checklist

Status: prepared 2026-10-29; execution is the release owner's step. This checklist is the
publication path for the private 0.1.0 candidate.

## Before publishing

- [ ] alint's plugin contract consumes the candidate and its CI is green against the generated
      plain Rust, proto, and glue. The candidate consumer branch is alint PR #409
      (`witforge-0.1-consumer-main`); its generated `alint-witforge-consumer` crate compiles and
      round-trips under the full alint CI matrix. Merge that PR before publication; the draft
      branch alone does not satisfy this release gate.
- [ ] `cargo fmt --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace --locked`
- [ ] `RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps`
- [ ] `cargo +1.88 test --workspace --locked`
- [ ] `ci/check-generated.sh`
- [ ] `ci/check-release-freeze.sh crates/witforge/tests/fixtures/versions/wit <merge-base>`
- [ ] `aplan check && aplan verify && aplan export-agents-md --check AGENTS.md && aplan check-docs`
      (local until a published aplan release lets CI run it)
- [ ] README status, `CHANGELOG.md`, and the workspace version say 0.1.0.
- [ ] The release branch has no unintended generated-file or fixture drift.

## Make the repository public

The self-hosted runner MUST NOT serve a public repository. The workflow selects GitHub-hosted
runners when `github.event.repository.private` is false, but the runner transition is still an
explicit release-owner action.

- [ ] Flip the GitHub repository visibility to public.
- [ ] Confirm public CI uses the Linux/macOS/Windows matrix in `.github/workflows/ci.yml`.
- [ ] Run `ci/runner/teardown.sh --purge` from the runner host, then verify the self-hosted
      runner is no longer registered: `gh api repos/asamarts/witforge/actions/runners`.
- [ ] Remove the `ci/runner/` installation from the host if it is no longer managed.
- [ ] Do not leave stored runner credentials or volumes behind.

## Publish

- [x] Set `publish = true` in `crates/witforge/Cargo.toml` and
      `crates/witforge-cli/Cargo.toml`.
- [x] Verify `cargo package -p witforge` and
      `cargo publish --dry-run -p witforge --allow-dirty`.
- [ ] `cargo publish -p witforge`
- [ ] After the library is visible on crates.io, verify `cargo package -p witforge-cli`.
      The CLI has a registry dependency on `witforge = "0.1.0"`, so it cannot be packaged before
      the library publish completes.
- [ ] `cargo publish -p witforge-cli`
- [ ] Tag the published commit `v0.1.0` and create the GitHub release with the changelog entry.
- [ ] Verify the published crates install:
      `cargo install witforge-cli --version 0.1.0 --locked`.

## After publishing

- [ ] Record the release commit and CI run in the changelog or release notes.
- [ ] Keep the released fixtures and generated-output checks in CI.
- [ ] Move the next contract change to a new minor version directory with `--against`.
