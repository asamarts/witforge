#!/usr/bin/env bash
# Test for ci/publish.sh --dry-run, in a scratch repository with an unpublished version: after a
# dry run, a second dry run of a changed library must verify the CLI against the change, not
# against the copy of the library cargo unpacked and built for the first run.
#
# It runs the working tree's ci/publish.sh on a copy of HEAD, needs the network (crates.io and
# its index), and builds the two crates twice in a scratch target directory.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION="0.1.0-publishtest"
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
sed -i.bak -e "s/^version = \"0\.1\.0\"$/version = \"${VERSION}\"/" Cargo.toml
sed -i.bak -e "s/^witforge = { path = \"..\/witforge\", version = \"0\.1\.0\" }$/witforge = { path = \"..\/witforge\", version = \"${VERSION}\" }/" \
    crates/witforge-cli/Cargo.toml
rm -f Cargo.toml.bak crates/witforge-cli/Cargo.toml.bak
grep -q "version = \"${VERSION}\"" Cargo.toml crates/witforge-cli/Cargo.toml \
    || { echo "FAIL: could not set the test version" >&2; exit 1; }
cargo update --workspace --quiet
git init -q
git config user.name test
git config user.email test@example.invalid
git add -A
git commit -qm "the library as first verified"

dry_run() {
    local output status=0
    output="$(ci/publish.sh --dry-run 2>&1)" || status=$?
    if [[ ${status} -ne 0 ]] || ! grep -qF "Uploading witforge-cli v${VERSION}" <<<"${output}"; then
        echo "FAIL: $1 (exit ${status}):" >&2
        tail -n 20 <<<"${output}" >&2
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
