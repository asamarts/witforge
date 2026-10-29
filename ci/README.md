# CI

While the repository is private, CI runs on one self-hosted Linux runner (labels `self-hosted`,
`linux`, `witforge`) instead of GitHub-hosted runners, whose minutes are billed for private
repositories. The workflow selects the self-hosted runtime only while
`github.event.repository.private` is true; once the repository is public it selects Ubuntu for
lint/docs/plan/generation and the full Linux/macOS/Windows matrix for tests. A self-hosted runner
must never serve a public repository, where pull requests from forks could run code on it, so the
release checklist retires this runner explicitly.

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
(`asamarts` by default; override with `WITFORGE_GH_USER` and `WITFORGE_REPO`).


## Gates

The workflow in `.github/workflows/ci.yml` runs the normal Rust jobs plus:

- **plan**: installs the pinned aplan archive from `ci/bin`, then runs `aplan check`,
  `aplan verify`, `aplan export-agents-md --check AGENTS.md`, and `aplan check-docs`.
- **generated**: builds the CLI, generates all three emitters into a temporary tree with
  `ci/check-generated.sh`, runs `--check` on each, and runs `ci/check-release-freeze.sh` against
  the versions fixture and the push/PR base commit.
- **buf** (workflow dispatch only): optional `buf breaking` against the generated proto output.
  It is disabled by default because the lockfile is the normative wire check; the manual input is
  exposed for consumers that also run `buf`.

The aplan binary is 0.5.0-dev, committed as `ci/bin/aplan.gz` with a checksum because no published
release with this plan surface exists yet. Replace both files together when such a release is
available. The runner is Linux x86_64 only while the repository is private; the archive is a
static musl build so it does not depend on the image's glibc.
