# Prebuilt releases and the witforge GitHub Action

Status: implemented 2026-11-05 (`W8-003`); the first release carrying these assets is 0.3.0.

Rust users can continue to install `witforge-cli` from crates.io. CI users and non-Rust
consumers can download a prebuilt archive from the matching GitHub release instead of building
the compiler.

## Release assets

Each `vX.Y.Z` release attaches one gzip-compressed tar archive per target:

| Platform | Target | Archive |
|---|---|---|
| Linux x86_64 (static musl, from 0.8.0) | `x86_64-unknown-linux-musl` | `witforge-X.Y.Z-x86_64-unknown-linux-musl.tar.gz` |
| Linux x86_64 (glibc >= 2.39) | `x86_64-unknown-linux-gnu` | `witforge-X.Y.Z-x86_64-unknown-linux-gnu.tar.gz` |
| Linux arm64 (static musl, from 0.8.0) | `aarch64-unknown-linux-musl` | `witforge-X.Y.Z-aarch64-unknown-linux-musl.tar.gz` |
| Linux arm64 (glibc >= 2.39, from 0.7.0) | `aarch64-unknown-linux-gnu` | `witforge-X.Y.Z-aarch64-unknown-linux-gnu.tar.gz` |
| macOS Intel | `x86_64-apple-darwin` | `witforge-X.Y.Z-x86_64-apple-darwin.tar.gz` |
| macOS Apple silicon | `aarch64-apple-darwin` | `witforge-X.Y.Z-aarch64-apple-darwin.tar.gz` |
| Windows x86_64 | `x86_64-pc-windows-msvc` | `witforge-X.Y.Z-x86_64-pc-windows-msvc.tar.gz` |

Every archive has a sibling `.sha256` file containing the archive's SHA-256 digest and filename,
and (from 0.7.0 onward) a GitHub Artifact Attestation (SLSA build provenance). The release workflow
verifies both the digest and provenance before publishing, and the Action verifies them again after
download. Each archive contains the executable (`witforge`, or `witforge.exe` on Windows) at its
root alongside `LICENSE-APACHE`, `LICENSE-MIT`, and `README.md`.

Linux binaries are built natively on Ubuntu 24.04 hosted runners (`ubuntu-latest` and
`ubuntu-24.04-arm`). Starting in 0.8.0, static `musl` archives (`x86_64-unknown-linux-musl` and
`aarch64-unknown-linux-musl`) are published alongside the `gnu` archives, and the Action selects
the static `musl` archive on Linux for releases `>= 0.8.0` (retaining `gnu` for `<= 0.7.x`) so
Linux containers and LTS distributions (Ubuntu 22.04, Debian 12, RHEL 9, Alpine) work without a
glibc 2.39 floor. Windows `arm64` builds are **not** provided; revisit if a consumer requests them.
An unsupported platform/architecture combination fails with an error naming the detected
platform/arch rather than running the wrong binary.

## Installing a prebuilt archive

```sh
VERSION=0.8.0
TARGET=x86_64-unknown-linux-musl
BASE="https://github.com/asamarts/witforge/releases/download/v${VERSION}"
curl -sSfLO "${BASE}/witforge-${VERSION}-${TARGET}.tar.gz"
curl -sSfLO "${BASE}/witforge-${VERSION}-${TARGET}.tar.gz.sha256"
sha256sum -c "witforge-${VERSION}-${TARGET}.tar.gz.sha256"
tar -xzf "witforge-${VERSION}-${TARGET}.tar.gz"
./witforge --version
```

On macOS, use `shasum -a 256 -c` where `sha256sum` is unavailable. On Windows, use the matching
archive and run `witforge.exe`.

## Action usage

The action is published from this repository. It installs the release matching the action tag
by default. Every GitHub API request, release-asset download, and `gh attestation verify`
invocation has a 30-second abort timeout, so a stalled network fails the step instead of hanging
until the job timeout. The HTTP timeout was added in 0.6.0 (Action releases through `v0.5.0` did
not bound requests), and the `gh attestation verify` timeout was added in 0.8.0.

The JavaScript entry point uses GitHub's Node 24 Action runtime and has no npm dependencies. CI
executes the checked-out action with `uses: ./` so changes to `action.yml` are covered before
publication, then executes a pinned published ref on Linux x86_64, Linux arm64, macOS arm64,
macOS Intel, and Windows x86_64. Running `action/test.js` alone does not exercise GitHub's runtime, remote tag
resolution, archive extraction, or PATH propagation.

```yaml
- uses: asamarts/witforge@v0.8.0 # pinned: installs witforge 0.8.0
  with:
    check: wit/v0.2
    diff-old: wit/v0.1
    diff-new: wit/v0.2
    config: witforge.toml
    generators: rust,proto
    github-token: ${{ github.token }} # required through v0.7.x; defaulted from v0.8.0
```

Floating refs such as `@main`, commit SHAs (`@<40-hex>`), and local `./` checkouts install the
workspace version recorded in `Cargo.toml` in the Action's own tree at that ref rather than
silently floating to `latest`. If that version has no published assets yet (for example on a
release-preparation branch), the Action fails and names the `version` input. Pass `version: latest`
only when floating latest is explicitly desired, or pass `version` to pin another release:

```yaml
- uses: asamarts/witforge@main # pins to the tree's version, or override explicitly:
  with:
    version: 0.8.0
```

Starting in `v0.8.0`, `github-token` defaults to `${{ github.token }}` so `gh attestation verify`
and explicit `version: latest` API lookups work out of the box without anonymous GitHub API rate
limits (releases through `v0.7.x` require passing `github-token: ${{ github.token }}` explicitly).
The token is never sent with public release-asset downloads. All inputs are validated before any
network request; transient network/5xx errors retry up to 3 attempts while HTTP 4xx and SHA-256
checksum mismatches are never retried. Downloaded archive and `.sha256` scratch files are removed
immediately after extracting the binary under `$RUNNER_TEMP`.

The runner exports an input as `INPUT_<NAME>` with the name upper-cased and spaces replaced by
underscores; a hyphen stays a hyphen, so `diff-old` is `INPUT_DIFF-OLD`. Action releases through
v0.5.0 replaced hyphens too and therefore read `diff-old`, `diff-new`, `diff-check`, and
`github-token` as empty: they never ran `diff`, and every latest-release lookup was anonymous.
`action/test.js` now reads every input declared in `action.yml` through the runner's names, and
CI runs the checked-out Action both on a breaking fixture pair (asserting `outputs.failed-command`
equals `witforge diff`, not `install`) and against the current commit's locally built binary via
`binary:`.

### Inputs and outputs

Every input and output is documented publicly on [`docs/guide/ci.md`](../guide/ci.md):

| Input | Default | Behavior |
|---|---|---|
| `version` | empty | `X.Y.Z`/`vX.Y.Z`, or `latest`. Empty uses the action ref tag or the Action tree's `Cargo.toml` version. Cannot be combined with `binary`. |
| `binary` | empty | Path to an existing local `witforge` executable, skipping download. Cannot be combined with `version`. |
| `check` | empty | Version directories or WIT roots for `witforge check`, one per line, or `true` (from `v0.8.0`) to check every project in `config`. |
| `against` | empty | Baseline `wire.lock` path (single `check` entry) or `<check-dir>=<baseline>` lines (multiple `check` entries) for `check --against`. Cannot be combined with `check: true`. |
| `diff-old` | empty | Old version directory or root for `witforge diff`. |
| `diff-new` | empty | New version directory or root for `witforge diff`. |
| `diff-check` | `true` | `true` (`--check`), `false` (report only), or comma-separated views (`wire`, `source`, `api`, `decode`). |
| `config` | empty | `witforge.toml` used for `check: true`, `freeze: true`, and configured generators. |
| `project` | empty | Project name passed to `check: true`, `freeze: true`, and configured generators. |
| `generators` | `rust` | Comma- or whitespace-separated `rust`, `proto`, `schema`, `glue` (each runs with `--config` and `--check`), or `all` (from `v0.8.0`) to run `witforge generate --config ... --check` across every configured generator and project. Empty skips generation (pass `generators: ""` when using `config` only for `check: true` or `freeze: true`). |
| `freeze` | empty | WIT roots to verify with `witforge freeze` (one per line), or `true` to verify every project in `config` (requires `fetch-depth: 0`). |
| `format` | `human` | `human`, `json`, or `sarif`. Failing checks always emit workflow annotations (`::error file=...,line=...,col=...::...`). |
| `sarif-file` | empty | Combined SARIF 2.1.0 output file path when `format: sarif` is set (default `witforge.sarif`). |
| `github-token` | `${{ github.token }}` (from `v0.8.0`) | Token for GitHub API calls (`version: latest` and `gh attestation verify`); defaults to the workflow token from `v0.8.0` and is never sent with asset downloads. |

Outputs: `failed-command` (`validate`, `install`, or `witforge <subcommand>` on failure; empty on
success) and `sarif-file` (path to the written SARIF file when `format: sarif` is set).

With `diff-old` and `diff-new` set and `diff-check` left at its default, the step fails on any
wire-, source-, or generated-API-breaking change. That includes an added record field, so a
workflow that only wants the report sets `diff-check: "false"` or `diff-check: "wire"`. Because
releases through v0.5.0 ignored these inputs, a workflow that already passes them starts gating
when its pin moves to `v0.6.0` or later.

The action leaves `witforge` on `PATH` and exports `WITFORGE_BIN` for later steps. It runs, in
order:

1. `check`: either `witforge check --config ...` (when `check: true`) or each explicit `check`
   entry (with `--against` where configured);
2. `diff` when both `diff-old` and `diff-new` are set;
3. `freeze` when enabled (either `witforge freeze --config ...` for `freeze: true` or each explicit
   WIT root);
4. configured generators: either `witforge generate --config ... --check` (when `generators: all`)
   or each listed generator with `--config` and `--check`.

### Checksums and supply chain

The action downloads the archive and its `.sha256` file over HTTPS (without credentials), verifies
the digest with Node's built-in SHA-256 implementation, verifies the GitHub build provenance
attestation with `gh attestation verify` (for releases 0.7.0+; warns and falls back to checksum
only if `gh` is absent from the runner), and only then extracts the executable and removes the
downloaded archive and checksum scratch files. It does not download or execute a shell installer,
and it has no npm dependencies. The action code itself is committed in [`action/`](../../action/),
and the release workflow pins its GitHub Actions to immutable commits.

## Release workflow

The `v*` tag workflow (`release.yml`) runs `verify -> binaries -> smoke-artifacts -> publish ->
github-release -> action-smoke`: platform archives are built for seven target archives across five
platforms (Linux x86_64 static `musl` and `gnu`, Linux arm64 static `musl` and `gnu`, macOS Intel,
macOS Apple silicon, Windows x86_64), attested with
`actions/attest-build-provenance`, and smoke-tested from workflow artifacts before anything is
published. `publish` verifies deterministic `.crate` digests and publishes with `--no-verify`;
`github-release` creates a draft release, uploads assets without `--clobber` (verifying existing
digests on re-run), and publishes the draft once all assets are attached. Main CI separately
invokes the pinned remote release ref across hosted targets, proving GitHub tag resolution rather
than only the checked-out Action.
