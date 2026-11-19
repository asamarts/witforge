#!/usr/bin/env bash
# Run ci/check-buf.sh for every fixture WIT root against every published release.
#
# The releases are this repository's `vX.Y.Z` tags, oldest first, and the fixtures are every
# `crates/witforge/tests/fixtures/*/wit` root, so neither list can fall behind what exists. A tag
# that points at HEAD is the release being made: its binary is not published yet, so it is
# skipped. ci/check-buf.sh skips a fixture that did not exist at a release.
#
# Every other tag must be installable with `cargo install witforge-cli --version X.Y.Z`. A
# release that is not (yanked, or tagged and never published) is listed in ci/buf-history-skip
# with the reason, and is reported and skipped; an unlisted one fails the run.
#
# Usage: [WITFORGE=path/to/candidate] [BUF=path/to/buf] [BUF_REQUIRED=1]
#        [WITFORGE_BASE_CACHE=dir] ci/check-buf-history.sh
#   The checkout needs its tags (`fetch-depth: 0` in a workflow). Each release is built once
#   per run unless WITFORGE_BASE_CACHE names a directory that already holds it.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${ROOT}"

# ci/buf-history-skip: one `vX.Y.Z reason` per line; `#` starts a comment.
skip_tags=()
skip_reasons=()
skip_file="${ROOT}/ci/buf-history-skip"
if [[ -f "${skip_file}" ]]; then
    number=0
    while IFS= read -r line || [[ -n "${line}" ]]; do
        number=$((number + 1))
        line="${line%%#*}"
        read -r tag reason <<<"${line}" || true
        [[ -n "${tag}" ]] || continue
        if [[ ! "${tag}" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ || -z "${reason}" ]]; then
            echo "ci/buf-history-skip:${number}: expected \`vX.Y.Z reason\`" >&2
            exit 1
        fi
        git rev-parse --verify --quiet "refs/tags/${tag}^{commit}" >/dev/null || {
            echo "ci/buf-history-skip:${number}: ${tag} is not a tag of this repository" >&2
            exit 1
        }
        skip_tags+=("${tag}")
        skip_reasons+=("${reason}")
    done <"${skip_file}"
fi

head="$(git rev-parse HEAD)"
releases=()
skipped=()
while IFS= read -r tag; do
    [[ "${tag}" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]] || continue
    commit="$(git rev-parse --verify "${tag}^{commit}")"
    [[ "${commit}" != "${head}" ]] || continue
    reason=""
    if [[ ${#skip_tags[@]} -gt 0 ]]; then
        for index in "${!skip_tags[@]}"; do
            [[ "${skip_tags[${index}]}" != "${tag}" ]] || reason="${skip_reasons[${index}]}"
        done
    fi
    if [[ -n "${reason}" ]]; then
        echo "skipping ${tag}: ${reason}"
        skipped+=("${tag}")
        continue
    fi
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
if [[ ${#skipped[@]} -gt 0 ]]; then
    echo "not compared (ci/buf-history-skip): ${skipped[*]}"
fi
