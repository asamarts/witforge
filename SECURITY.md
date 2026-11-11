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
- Release binaries and archives are produced by the tagged release workflow from the checked
  commit, with SHA-256 checksums attached to each archive.
- The GitHub Action verifies the downloaded archive checksum before extracting it.
- Generated output and release freezes are checked separately in CI; see
  [`docs/design/quality.md`](docs/design/quality.md).
