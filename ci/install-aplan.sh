#!/usr/bin/env bash
# Install the pinned aplan binary used by CI.
#
# The archive is committed because aplan 0.5.0-dev is not published as a release yet. The file is
# a static x86_64 Linux build; the pinned checksum is verified before use. Replace the archive and
# checksum together when a published 0.5 release is available.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ARCHIVE="${ROOT}/ci/bin/aplan.gz"
CHECKSUM_FILE="${ROOT}/ci/bin/aplan.sha256"
DEST="${1:-${RUNNER_TEMP:-${TMPDIR:-/tmp}}/aplan}"

[[ -f "${ARCHIVE}" ]] || { echo "missing ${ARCHIVE}" >&2; exit 1; }
[[ -f "${CHECKSUM_FILE}" ]] || { echo "missing ${CHECKSUM_FILE}" >&2; exit 1; }
EXPECTED="$(tr -d '[:space:]' < "${CHECKSUM_FILE}")"
ACTUAL="$(sha256sum "${ARCHIVE}" | awk '{print $1}')"
[[ -n "${EXPECTED}" && "${ACTUAL}" == "${EXPECTED}" ]] || {
    echo "aplan archive checksum mismatch: expected ${EXPECTED}, got ${ACTUAL}" >&2
    exit 1
}
mkdir -p "$(dirname "${DEST}")"
gzip -dc "${ARCHIVE}" > "${DEST}"
chmod 0755 "${DEST}"
VERSION="$("${DEST}" --version)"
[[ "${VERSION}" == "aplan 0.5.0-dev" ]] || {
    echo "unexpected aplan version: ${VERSION}" >&2
    exit 1
}
echo "installed ${VERSION} at ${DEST}"
