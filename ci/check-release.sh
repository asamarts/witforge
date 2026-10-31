#!/usr/bin/env bash
# Check that the workspace is ready to release, and print its version.
#
# Usage: ci/check-release.sh [TAG]
#   Both crates must have the same version, and CHANGELOG.md must have a dated section for it
#   (`## [X.Y.Z] - YYYY-MM-DD`) with at least one entry. With TAG, the tag must be `vX.Y.Z`
#   and point at HEAD, and HEAD must be on origin/main.
#   The release notes (that section, without its heading) are written to RELEASE_NOTES when it
#   is set.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${ROOT}"
TAG="${1:-}"

fail() {
    echo "release check: $*" >&2
    exit 1
}

metadata="$(cargo metadata --no-deps --format-version 1 --locked)"
versions="$(jq -r '.packages[] | select(.name == "witforge" or .name == "witforge-cli") | .version' \
    <<<"${metadata}" | sort -u)"
[[ "$(wc -l <<<"${versions}")" -eq 1 && -n "${versions}" ]] \
    || fail "witforge and witforge-cli must have one version, found: ${versions//$'\n'/, }"
version="${versions}"

escaped="${version//./\\.}"
heading="$(grep -E "^## \[${escaped}\]" CHANGELOG.md || true)"
[[ -n "${heading}" ]] || fail "CHANGELOG.md has no section for ${version}"
[[ "$(wc -l <<<"${heading}")" -eq 1 ]] || fail "CHANGELOG.md has more than one section for ${version}"
dated="^## \[${escaped}\] - [0-9]{4}-[0-9]{2}-[0-9]{2}$"
[[ "${heading}" =~ ${dated} ]] || fail "the CHANGELOG.md heading for ${version} has no date: ${heading}"
date="${heading##* - }"
[[ "$(date -u -d "${date}" +%F 2>/dev/null)" == "${date}" ]] \
    || fail "the CHANGELOG.md date for ${version} is not a valid date: ${date}"

notes="$(awk -v heading="${heading}" '
    $0 == heading { inside = 1; next }
    inside && /^## \[/ { exit }
    inside { print }
' CHANGELOG.md | sed -e '/./,$!d' | sed -e ':a' -e '/^\n*$/{$d;N;ba' -e '}')"
grep -q '^- ' <<<"${notes}" || fail "the CHANGELOG.md section for ${version} has no entries"
if [[ -n "${RELEASE_NOTES:-}" ]]; then
    printf '%s\n' "${notes}" >"${RELEASE_NOTES}"
fi

if [[ -n "${TAG}" ]]; then
    [[ "${TAG}" == "v${version}" ]] || fail "tag ${TAG} does not match the version ${version}"
    [[ "$(git rev-parse --verify --quiet "refs/tags/${TAG}^{commit}")" == "$(git rev-parse HEAD)" ]] \
        || fail "tag ${TAG} does not exist or does not point at HEAD"
    git merge-base --is-ancestor HEAD origin/main \
        || fail "${TAG} is not on origin/main; release from main"
fi

echo "${version}"
