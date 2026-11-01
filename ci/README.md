# CI

CI runs on GitHub-hosted runners: Ubuntu for lint, docs, and generation, and the full
Linux/macOS/Windows matrix for tests. Before 0.1.0, while the repository was private, it ran on
one self-hosted Linux runner (labels `self-hosted`, `linux`, `witforge`), because hosted minutes
are billed for private repositories. The workflow still selects that runner only when
`github.event.repository.private` is true. A self-hosted runner must never serve a public
repository, where pull requests from forks could run code on it, so the release checklist retired
it when the repository went public; the setup below is kept for reference.

## The runner

A rootless podman container managed as a systemd user service through quadlet:

- `ci/Containerfile`: Debian with Rust stable (plus clippy and rustfmt) and 1.88 for the MSRV
  job, and the GitHub Actions runner agent, checksum-verified.
- `ci/runner/entrypoint.sh`: registers once (`register`), or runs the agent with the stored
  credentials. The long-running service never holds a token.
- `ci/runner/setup.sh`: builds the image, registers the runner if no registration is stored
  (minting a one-hour registration token with `gh`), and installs and restarts
  `witforge-runner.service`. Re-run it after changing the image.
- `ci/runner/teardown.sh`: stops the service and removes its unit; `--purge` also deregisters
  the runner and deletes its volumes and image.

The container is capped at 4 CPUs and 8 GiB of memory, and keeps two volumes: the registration
(`witforge-runner-config`) and the cargo registry cache (`witforge-runner-cargo`). Each job
checks out and builds from scratch, which takes well under a minute.

```sh
ci/runner/setup.sh                       # install or update
journalctl --user -fu witforge-runner    # agent log
ci/runner/teardown.sh [--purge]          # remove
```

Both scripts need podman and the `gh` CLI logged in to the account that owns the repository
(`asamarts` by default; override with `WITFORGE_GH_USER` and `WITFORGE_REPO`). `teardown.sh
--purge` refuses to change anything unless the runner is registered on `WITFORGE_REPO`.

## Gates

The workflow in `.github/workflows/ci.yml` runs the normal Rust jobs plus:

- **workflows and scripts**: pinned, checksum-verified actionlint and shellcheck (the runner
  image's shellcheck changes version, and with it findings and codes) over both workflows, with
  shellcheck's masked-return check enabled for every `run:` script (a `$(...)` whose failure
  would not fail its step, as in `echo "x=$(cmd)"`), and shellcheck over `ci/`.
- **generated**: builds the CLI and runs `ci/check-generated.sh`, which generates all three
  emitters' output for the versions fixture into a temporary tree and runs `--check` on each
  against it, so generation must succeed and reproduce itself byte for byte (the golden tests
  pin the output's content). Then `ci/check-release-freeze.sh` on the versions fixture: it reads
  the full history of `released`, merges included, so an entry once committed can never be
  removed and a released directory can never differ from any commit that listed it, whatever
  the push or pull request. `ci/test-release-freeze.sh` tests that script; the test jobs run it
  on Linux and, with the stock bash 3.2, on macOS.
- **package**: `cargo publish --workspace --dry-run`, which packages and verifies both crates,
  and `ci/test-publish.sh`, which checks that a repeated `ci/publish.sh --dry-run` verifies the
  CLI against the current library rather than a copy cargo cached for an earlier run.
- **buf** (workflow dispatch only): optional `buf breaking`, comparing the protobuf output
  generated at the base commit with the output generated now. It is off by default because the
  lockfile is the normative wire check; the manual input is exposed for consumers that also run
  `buf`. The job installs a pinned `buf` and fails if it cannot run it.

The aplan plan (`aplan check`, `aplan verify`, `aplan export-agents-md --check AGENTS.md`, and
`aplan check-docs`) is checked locally, as the release checklist says, until a published aplan
release with this plan surface exists; CI then gains a `plan` job that installs that release.

## Releases

`.github/workflows/release.yml` runs when a `vX.Y.Z` tag is pushed:

- **verify**: `ci/check-release.sh` requires one version for both crates, a dated
  `CHANGELOG.md` section with entries for it, and a tag that matches the version, points at the
  checked-out commit, and is on `main`. Then the tests and `ci/publish.sh --dry-run`.
- **publish** (environment `release`, which only `v*` tags may use): publishes, library first,
  each crate whose version is not on crates.io yet (`ci/publish.sh`), with a crates.io token
  minted through trusted publishing for that job. Nothing to publish skips the token request.
- **github-release**: creates the GitHub release from the changelog section, unless it exists.

A manual run (`gh workflow run release.yml`) runs only **verify**, on the selected branch.

Each crate trusts this repository, `release.yml`, and the `release` environment on crates.io
(the crate's Settings, Trusted Publishing). crates.io cannot do that for a crate that does not
exist yet, so a new crate's first version is published by hand with a short-lived API token
in `CARGO_REGISTRY_TOKEN` (`ci/publish.sh`), after which its trusted publisher is added and the
token revoked; the release checklist has the steps.
