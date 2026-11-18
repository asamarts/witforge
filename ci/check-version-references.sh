#!/usr/bin/env bash
# Check that current-release references in documentation and the Action agree with a version.
#
# Usage: ci/check-version-references.sh [ROOT] [VERSION]
#
# The checker deliberately names the current-version patterns instead of scanning every
# occurrence of a version string: historical design notes, the changelog, and release records
# legitimately mention older releases.
set -euo pipefail

ROOT="${1:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
VERSION="${2:-}"

fail() {
    echo "version reference check: $*" >&2
    exit 1
}

[[ -n "${VERSION}" ]] || fail "missing expected version"
[[ "${VERSION}" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || fail "invalid expected version: ${VERSION}"
cd "${ROOT}"

check_pattern() {
    local file="$1"
    local regex="$2"
    local label="$3"
    local found=0
    local number=0
    local line
    [[ -f "${file}" ]] || fail "missing ${file}"

    while IFS= read -r line || [[ -n "${line}" ]]; do
        number=$((number + 1))
        if [[ "${line}" =~ ${regex} ]]; then
            found=1
            local value="${BASH_REMATCH[1]}"
            [[ "${value}" == "${VERSION}" ]] \
                || fail "${file}:${number}: ${label} names ${value}; expected ${VERSION}"
        fi
    done <"${file}"
    [[ "${found}" -eq 1 ]] || fail "${file}: no ${label} found"
}

current_release='Current release: \*\*([0-9]+\.[0-9]+\.[0-9]+)\*\*'
install_version='cargo install witforge-cli --version ([0-9]+\.[0-9]+\.[0-9]+)'
action_ref='asamarts/witforge@v([0-9]+\.[0-9]+\.[0-9]+)'
action_comment='installs witforge ([0-9]+\.[0-9]+\.[0-9]+)'
action_version='VERSION=([0-9]+\.[0-9]+\.[0-9]+)'
action_input='[[:space:]]version: ([0-9]+\.[0-9]+\.[0-9]+)'

check_pattern README.md "${current_release}" "current release"
check_pattern docs/index.md "${current_release}" "current release"
check_pattern README.md "${install_version}" "install version"
check_pattern docs/quickstart.md "${install_version}" "install version"
check_pattern README.md "${action_ref}" "Action reference"
check_pattern docs/guide/ci.md "${action_ref}" "Action reference"
check_pattern docs/design/action.md "${action_ref}" "Action reference"
check_pattern docs/design/action.md "${action_comment}" "Action reference comment"
check_pattern docs/design/action.md "${action_version}" "Action version example"
check_pattern docs/design/action.md "${action_input}" "Action input example"
check_pattern .github/workflows/ci.yml "${action_ref}" "Action reference"
check_pattern .github/workflows/ci.yml "${action_input}" "Action input"

echo "version references match ${VERSION}"
