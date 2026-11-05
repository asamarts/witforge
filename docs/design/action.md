# Prebuilt releases and the witforge GitHub Action

Status: implemented 2026-11-05 (`W8-003`); the first release carrying these assets is 0.3.0.

Rust users can continue to install `witforge-cli` from crates.io. CI users and non-Rust
consumers can download a prebuilt archive from the matching GitHub release instead of building
the compiler.

## Release assets

Each `vX.Y.Z` release attaches one gzip-compressed tar archive per target:

| Platform | Target | Archive |
|---|---|---|
| Linux x86_64 | `x86_64-unknown-linux-gnu` | `witforge-X.Y.Z-x86_64-unknown-linux-gnu.tar.gz` |
| macOS Intel | `x86_64-apple-darwin` | `witforge-X.Y.Z-x86_64-apple-darwin.tar.gz` |
| macOS Apple silicon | `aarch64-apple-darwin` | `witforge-X.Y.Z-aarch64-apple-darwin.tar.gz` |
| Windows x86_64 | `x86_64-pc-windows-msvc` | `witforge-X.Y.Z-x86_64-pc-windows-msvc.tar.gz` |

Every archive has a sibling `.sha256` file containing the archive's SHA-256 digest and filename.
The release workflow verifies the digest before uploading, and the Action verifies it again after
download. The archive contains the executable (`witforge`, or `witforge.exe` on Windows) at its
root.

The macOS archives are built for both architectures by the release job; the Linux and Windows
assets are x86_64. An unsupported platform/architecture combination fails with a named error
rather than running the wrong binary.

## Installing a prebuilt archive

```sh
VERSION=0.3.0
TARGET=x86_64-unknown-linux-gnu
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
by default:

```yaml
- uses: asamarts/witforge@v0.3.0 # pinned: installs witforge 0.3.0
  with:
    check: wit/v0.2
    diff-old: wit/v0.1
    diff-new: wit/v0.2
    config: witforge.toml
    generators: rust,proto
```

Floating refs such as `@main` have no version in the action ref, so they install the latest
published release. A `version` input overrides either default:

```yaml
- uses: asamarts/witforge@main # floats, but installs a specific release
  with:
    version: 0.3.0
```

Pass `${{ github.token }}` as `github-token` when querying the latest release to avoid anonymous
GitHub API rate limits; pinned versions do not query the API.

### Inputs

| Input | Default | Behavior |
|---|---|---|
| `version` | empty | `X.Y.Z`/`vX.Y.Z`, or `latest`. Empty uses the action ref when it is a version tag, otherwise latest. |
| `check` | empty | Version directories for `witforge check`, one per line. |
| `diff-old` | empty | Old version directory or root for `witforge diff`. |
| `diff-new` | empty | New version directory or root for `witforge diff`. |
| `diff-check` | `true` | Pass `--check`, failing the step on breaking changes. Set `false` to report only. |
| `config` | empty | `witforge.toml` used for generate/check runs. |
| `project` | empty | Project name passed to each generator. |
| `generators` | `rust` | Comma- or whitespace-separated `rust`, `proto`, `schema`, `glue`; each runs with `--config` and `--check`. Empty skips generation. |
| `github-token` | empty | Token for the latest-release lookup. |

The action leaves `witforge` on `PATH` and exports `WITFORGE_BIN` for later steps. It runs, in
order:

1. every `check` entry;
2. `diff` when both old and new are set;
3. every configured generator with `--check`.

### Checksums and supply chain

The action downloads the archive and its `.sha256` file over HTTPS, verifies the digest with
Node's built-in SHA-256 implementation, and only then extracts the executable. It does not
download or execute a shell installer, and it has no npm dependencies. The action code itself is
committed in [`action/`](../../action/), and the release workflow pins its GitHub Actions to
immutable commits.

## Release workflow

The `v*` tag workflow runs after the GitHub release is created. Its binary matrix builds the
four targets, packages each with `ci/release-asset.sh`, verifies the generated checksum, and
uploads the archive and checksum with `gh release upload --clobber`, so a rerun repairs a partial
upload. The crates.io publish remains a separate, earlier job.
