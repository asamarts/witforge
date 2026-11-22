# Security policy

## Supported versions

Security fixes are applied to the latest stable release line: the latest 0.x release until 1.0,
then the latest 1.x release. Older 0.x releases are unsupported after 1.0. An older release may
receive a fix only when the issue affects a supported consumer and a backport is safe; that is
an exception, not a support promise.

## Reporting a vulnerability

Please report vulnerabilities **privately** through GitHub's private vulnerability reporting
for [`asamarts/witforge`](https://github.com/asamarts/witforge/security/advisories/new). Do not
open a public issue, pull request, discussion, or CI log for a suspected vulnerability.

Include:

- the affected version or commit;
- a minimal WIT package, command, or generated-output case that reproduces the issue;
- the expected and actual behavior;
- whether the issue can affect generated code, a release artifact, or the GitHub Action.

We will acknowledge a report, investigate it, and coordinate disclosure and a fix. Please allow
time for a fix before public disclosure.

## Threat model

witforge is frequently run in CI against untrusted pull-request changes to `.wit` files,
`wire.lock`, `wit/released`, and `witforge.toml`:

- **Untrusted WIT and lockfile input:** The compiler parses WIT packages and lockfiles without
  executing code from them. Type nesting (32 levels), alias chains (32 links), canonical type
  strings (1024 bytes), and synthesized wrapper names (1024 bytes) are bounded so adversarial
  inputs fail cleanly as contract or parse errors rather than exhausting stack or memory.
- **Filesystem write boundaries:** Generators refuse any output root that coincides with, nests
  inside, or contains the WIT root or any directory listed in `wit/released` (resolving symlinks
  and `..` components before writing). Every generated file carries a contract- and
  generator-scoped ownership header; generators refuse symlinks, directories, foreign-owner files,
  and unowned files at expected output paths, leave unrelated unowned files untouched, and replace
  files atomically via process-unique temporary siblings. `witforge lock` refuses to modify any
  released directory or write a lockfile inside one, and refuses symlinks inside listed released
  directories.
- **Network isolation:** The `witforge` library and CLI open no network connections.
- **GitHub Action credentials:** `github-token` is sent only to GitHub API calls (latest-release
  resolution when explicitly requested, and `gh attestation verify`). Release archives and `.sha256`
  files are public downloads fetched without credentials, verified by SHA-256 and (from 0.7.0)
  build provenance attestation before extraction.

## Dependency and artifact policy

- `cargo audit` checks `Cargo.lock` against the RustSec advisory database in CI.
- `cargo deny check advisories bans licenses sources` enforces the configuration in
  [`ci/deny.toml`](ci/deny.toml), including known advisories, yanked crates, wildcard
  dependencies, and unknown registries or Git sources.
- Release binaries and archives are built, attested with GitHub Artifact Attestations (SLSA v1
  build provenance via `actions/attest-build-provenance`, starting with 0.7.0), and smoke-tested
  from workflow artifacts before any crate or GitHub release is published.
- GitHub releases are created as drafts, populated without overwriting existing assets (digest
  verified on re-runs), and published only once every platform archive and `.sha256` checksum is
  attached.
- Crates are published to crates.io via short-lived OIDC trusted publishing with `--no-verify`
  after matching the deterministic `.crate` SHA-256 digests produced in the unprivileged
  verification job, so the publishing job executes no build scripts from dependencies.
- Generated output and release freezes are checked separately in CI; see
  [`docs/design/quality.md`](docs/design/quality.md).

## Verifying a downloaded archive

Every release archive (`witforge-<version>-<target>.tar.gz`) carries a sibling `.sha256` file and
(from 0.7.0 onward) a Sigstore/GitHub build provenance attestation signed by
`asamarts/witforge/.github/workflows/release.yml`:

```sh
sha256sum -c "witforge-${VERSION}-${TARGET}.tar.gz.sha256"
gh attestation verify "witforge-${VERSION}-${TARGET}.tar.gz" \
  --repo asamarts/witforge \
  --signer-workflow asamarts/witforge/.github/workflows/release.yml
```

On macOS, use `shasum -a 256 -c` where `sha256sum` is unavailable.

The GitHub Action verifies the SHA-256 checksum on every release and runs `gh attestation verify`
on releases 0.7.0 and later before extracting the binary. If `gh` is not installed on a container
or self-hosted runner, the Action emits a warning and falls back to the verified SHA-256 checksum;
if `gh` is present and verification fails, the Action aborts without extracting the archive.
