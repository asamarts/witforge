#!/usr/bin/env bash
# Test for ci/publish.sh --dry-run, in a scratch repository with an unpublished version: after a
# dry run, a second dry run of a changed library must verify the CLI against the change, not
# against the copy of the library cargo unpacked and built for the first run.
#
# It runs the working tree's ci/publish.sh on a copy of HEAD, needs the network (crates.io and
# its index), and builds the two crates twice in a scratch target directory.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
metadata="$(cargo metadata --manifest-path "${ROOT}/Cargo.toml" --no-deps --format-version 1 --locked)"
BASE="$(jq -r '.packages[] | select(.name == "witforge") | .version' <<<"${metadata}")"
[[ -n "${BASE}" ]] || { echo "FAIL: no witforge version in cargo metadata" >&2; exit 1; }
# A version far above any release: crates.io reports it missing, and the CLI's requirement on
# the library matches only the temporary registry's copy. (A pre-release of the workspace
# version would also match the published release, which sorts above it.)
VERSION="9999.0.0-publishtest"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/witforge-publish.XXXXXX")"
cargo_home="${CARGO_HOME:-${HOME}/.cargo}"
cleanup() {
    rm -rf "${TMP}"
    # The temporary registry's unpacked copies of the test version, and their directories once
    # empty.
    local dir
    for dir in "${cargo_home}/registry/src/"-*/; do
        [[ -d "${dir}" ]] || continue
        rm -rf "${dir}witforge-${VERSION}" "${dir}witforge-cli-${VERSION}"
        rmdir "${dir}" 2>/dev/null || true
    done
}
trap cleanup EXIT

git -C "${ROOT}" archive HEAD | tar -x -C "${TMP}"
cp "${ROOT}/ci/publish.sh" "${TMP}/ci/publish.sh"
cd "${TMP}"
escaped="${BASE//./\\.}"
sed -i.bak -e "s/^version = \"${escaped}\"$/version = \"${VERSION}\"/" Cargo.toml
sed -i.bak -e "s/^witforge = { path = \"..\/witforge\", version = \"${escaped}\" }$/witforge = { path = \"..\/witforge\", version = \"${VERSION}\" }/" \
    crates/witforge-cli/Cargo.toml
rm -f Cargo.toml.bak crates/witforge-cli/Cargo.toml.bak
for manifest in Cargo.toml crates/witforge-cli/Cargo.toml; do
    grep -q "version = \"${VERSION}\"" "${manifest}" \
        || { echo "FAIL: could not set the test version in ${manifest}" >&2; exit 1; }
done
cargo update --workspace --quiet
git init -q
git config user.name test
git config user.email test@example.invalid
git add -A
git commit -qm "the library as first verified"

dry_run() {
    local output status=0
    # Plain output, so the match below works when CI sets CARGO_TERM_COLOR=always.
    output="$(CARGO_TERM_COLOR=never ci/publish.sh --dry-run 2>&1)" || status=$?
    if [[ ${status} -ne 0 ]] || ! grep -qF "Uploading witforge-cli v${VERSION}" <<<"${output}"; then
        echo "FAIL: $1 (exit ${status}):" >&2
        tail -n 20 <<<"${output}" >&2
        exit 1
    fi
    # The CLI is verified against the library just packaged, not a release from crates.io.
    local foreign
    foreign="$(grep -E '^ *Compiling witforge v' <<<"${output}" | grep -vF "witforge v${VERSION}" || true)"
    if [[ -n "${foreign}" ]]; then
        echo "FAIL: $1: verified against another witforge: ${foreign}" >&2
        exit 1
    fi
    echo "ok: $1"
}

dry_run "the first dry run verifies both crates"

# A library change the CLI depends on: a dry run against the first run's library fails to build.
cat >>crates/witforge/src/lib.rs <<'EOF'

/// Added by ci/test-publish.sh.
pub const PUBLISH_TEST_PROBE: u8 = 1;
EOF
cat >>crates/witforge-cli/src/main.rs <<'EOF'

const _: u8 = witforge::PUBLISH_TEST_PROBE;
EOF
git commit -qam "change the library"

dry_run "a second dry run verifies the CLI against the changed library"
echo "publish tests passed"
