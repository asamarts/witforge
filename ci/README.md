# CI

While the repository is private, CI runs on one self-hosted Linux runner (labels `self-hosted`,
`linux`, `witforge`) instead of GitHub-hosted runners, whose minutes are billed for private
repositories. macOS and Windows are not covered until the repository goes public; then the
workflow moves back to GitHub-hosted runners with the full three-OS matrix, and this runner is
torn down (a self-hosted runner must never serve a public repository, where pull requests from
forks could run code on it).

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
