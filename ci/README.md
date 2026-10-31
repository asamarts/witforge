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

- **generated**: builds the CLI, generates all three emitters into a temporary tree with
  `ci/check-generated.sh`, runs `--check` on each, and runs `ci/check-release-freeze.sh` against
  the versions fixture and the push/PR base commit.
- **buf** (workflow dispatch only): optional `buf breaking`, comparing the protobuf output
  generated at the base commit with the output generated now. It is off by default because the
  lockfile is the normative wire check; the manual input is exposed for consumers that also run
  `buf`. The job installs a pinned `buf` and fails if it cannot run it.

The aplan plan (`aplan check`, `aplan verify`, `aplan export-agents-md --check AGENTS.md`, and
`aplan check-docs`) is checked locally, as the release checklist says, until a published aplan
release with this plan surface exists; CI then gains a `plan` job that installs that release.
