#!/usr/bin/env bash
# Run ci/check-buf.sh for every fixture WIT root against every published release.
#
# The releases are this repository's `vX.Y.Z` tags, oldest first, and the fixtures are every
# `crates/witforge/tests/fixtures/*/wit` root, so neither list can fall behind what exists. A tag
# that points at HEAD is the release being made: its binary is not published yet, so it is
# skipped. ci/check-buf.sh skips a fixture that did not exist at a release.
#
# Usage: [WITFORGE=path/to/candidate] [BUF=path/to/buf] [BUF_REQUIRED=1]
#        [WITFORGE_BASE_CACHE=dir] ci/check-buf-history.sh
#   The checkout needs its tags (`fetch-depth: 0` in a workflow).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${ROOT}"

head="$(git rev-parse HEAD)"
releases=()
while IFS= read -r tag; do
    [[ "${tag}" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]] || continue
    commit="$(git rev-parse --verify "${tag}^{commit}")"
    [[ "${commit}" != "${head}" ]] || continue
    releases+=("${tag}")
done <<<"$(git tag --list 'v*' --sort=v:refname)"
if [[ ${#releases[@]} -eq 0 ]]; then
    echo "no published release tag found; the checkout needs its tags (git fetch --tags)" >&2
    exit 1
fi

fixtures=()
for root in crates/witforge/tests/fixtures/*/wit; do
    [[ -d "${root}" ]] || continue
    fixtures+=("${root}")
done
if [[ ${#fixtures[@]} -eq 0 ]]; then
    echo "no fixture WIT root found under crates/witforge/tests/fixtures" >&2
    exit 1
fi

for release in "${releases[@]}"; do
    for fixture in "${fixtures[@]}"; do
        "${ROOT}/ci/check-buf.sh" "${fixture}" "${release}"
    done
done
echo "buf breaking: ${#fixtures[@]} fixture root(s) against ${#releases[@]} release(s): ${releases[*]}"
