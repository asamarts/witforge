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
