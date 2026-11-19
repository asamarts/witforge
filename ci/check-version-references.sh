#!/usr/bin/env bash
# Check that current-release references in documentation and CI agree with a version.
#
# Usage: ci/check-version-references.sh ROOT VERSION
#
# Documentation names VERSION, the workspace version: it is written with the release it
# describes. CI's Action references are different, because CI executes them: they install a
# published release, and on the commit that prepares VERSION no `vVERSION` tag or asset exists
# yet. Until VERSION is published, `.github/workflows/ci.yml` may therefore name VERSION or the
# release before it (the next dated heading in ROOT/CHANGELOG.md), and must name one release
# throughout. VERSION counts as published once ROOT's repository has a `vVERSION` tag on a
# commit other than HEAD; from then on the workflow must name VERSION, which is the release
# checklist's step after publishing. A tag at HEAD is the release being made. A checkout without
# its tags cannot tell, and accepts the previous release.
#
# The checker deliberately names the current-version patterns instead of scanning every
# occurrence of a version string: historical design notes, the changelog, and release records
# legitimately mention older releases.
set -euo pipefail

fail() {
    echo "version reference check: $*" >&2
    exit 1
}

[[ $# -eq 2 ]] || fail "usage: ci/check-version-references.sh ROOT VERSION"
ROOT="$1"
VERSION="$2"
[[ "${VERSION}" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || fail "invalid expected version: ${VERSION}"
cd "${ROOT}"

# The release before VERSION: the first dated changelog heading after VERSION's own. Empty when
# VERSION has no dated heading or is the first release, and then only VERSION is accepted.
# WHY_NOT_PREVIOUS says why an older release in the workflow is refused, so that the message
# does not send the reader to pin CI to a release that does not exist yet.
PREVIOUS=""
WHY_NOT_PREVIOUS=""
if [[ ! -f CHANGELOG.md ]]; then
    WHY_NOT_PREVIOUS="there is no CHANGELOG.md to name the release before ${VERSION}"
elif ! grep -Eq "^## \\[${VERSION//./\\.}\\] - " CHANGELOG.md; then
    WHY_NOT_PREVIOUS="CHANGELOG.md has no dated section for ${VERSION}; date its heading, and CI may stay on the release before it until ${VERSION} is published"
fi
if [[ -f CHANGELOG.md ]]; then
    PREVIOUS="$(awk -v version="${VERSION}" '
        /^## \[[0-9]+\.[0-9]+\.[0-9]+\] - / {
            heading = $2
            sub(/^\[/, "", heading)
            sub(/\]$/, "", heading)
            if (seen) { print heading; exit }
            if (heading == version) seen = 1
        }' CHANGELOG.md)"
fi
if [[ -n "${PREVIOUS}" && -e .git ]]; then
    tagged="$(git rev-parse --verify --quiet "refs/tags/v${VERSION}^{commit}" 2>/dev/null || true)"
    if [[ -n "${tagged}" && "${tagged}" != "$(git rev-parse --verify --quiet HEAD 2>/dev/null || true)" ]]; then
        WHY_NOT_PREVIOUS="v${VERSION} is published (its tag is on an earlier commit), so CI must run it; the release checklist's last step moves the pinned Action"
        PREVIOUS=""
    fi
fi

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

# Like check_pattern, for a workflow that runs a published release: every match must name
# VERSION or PREVIOUS, and all matches of all patterns in the file must name the same release.
WORKFLOW_RELEASE=""
check_workflow_pattern() {
    local file="$1"
    local regex="$2"
    local label="$3"
    local found=0
    local number=0
    local line
    local expected="${VERSION}"
    [[ -z "${PREVIOUS}" ]] || expected="${VERSION} or the previous release ${PREVIOUS}"
    [[ -z "${WHY_NOT_PREVIOUS}" ]] || expected="${VERSION}: ${WHY_NOT_PREVIOUS}"
    [[ -f "${file}" ]] || fail "missing ${file}"

    while IFS= read -r line || [[ -n "${line}" ]]; do
        number=$((number + 1))
        if [[ "${line}" =~ ${regex} ]]; then
            found=1
            local value="${BASH_REMATCH[1]}"
            [[ "${value}" == "${VERSION}" || ( -n "${PREVIOUS}" && "${value}" == "${PREVIOUS}" ) ]] \
                || fail "${file}:${number}: ${label} names ${value}; expected ${expected}"
            [[ -z "${WORKFLOW_RELEASE}" || "${value}" == "${WORKFLOW_RELEASE}" ]] \
                || fail "${file}:${number}: ${label} names ${value}, but the file also names ${WORKFLOW_RELEASE}; use one release"
            WORKFLOW_RELEASE="${value}"
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
check_workflow_pattern .github/workflows/ci.yml "${action_ref}" "Action reference"
check_workflow_pattern .github/workflows/ci.yml "${action_input}" "Action input"

if [[ "${WORKFLOW_RELEASE}" == "${VERSION}" ]]; then
    echo "version references match ${VERSION}"
else
    echo "version references match ${VERSION}; CI runs the published ${WORKFLOW_RELEASE} Action"
fi
