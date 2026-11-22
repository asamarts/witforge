## Summary

- What changed and why?
- Related issue or `aplan` task ID (if applicable; contributors without `aplan` can leave the task ID blank and a maintainer will link one):

## Checklist

- [ ] Ran local checks with disk-backed scratch (`TMPDIR="$HOME/.tmp"`):
  - `cargo fmt --check`
  - `cargo clippy --workspace --all-targets -- -D warnings`
  - `RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps`
  - `TMPDIR="$HOME/.tmp" cargo test --workspace --locked`
- [ ] No released contract directories listed in `wit/released` were modified in place.
- [ ] If generated output or CLI flags changed: updated golden files / committed `examples/` (`ci/check-generated.sh`), updated `docs/guide/reference.md`, and noted migration impact under `## [Unreleased]` in `CHANGELOG.md`.
- [ ] If `aplan` is installed locally: `aplan check`, `aplan verify`, `aplan export-agents-md --check AGENTS.md`, and `aplan check-docs` pass.
