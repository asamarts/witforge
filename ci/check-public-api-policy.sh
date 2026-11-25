#!/usr/bin/env bash
# Enforce the reviewed 0.6.0 -> 0.7.0 API delta through the tag, then protect the tagged surface.
set -euo pipefail

ROOT="${1:?usage: check-public-api-policy.sh ROOT ACTUAL_INVENTORY}"
ACTUAL="${2:?usage: check-public-api-policy.sh ROOT ACTUAL_INVENTORY}"
ROOT="$(cd "${ROOT}" && pwd)"
TAG="${PUBLIC_API_BREAK_TAG:-v0.7.0}"
SCRATCH_BASE="${TMPDIR:-${HOME}/.tmp}"
mkdir -p "${SCRATCH_BASE}"
SCRATCH="$(mktemp -d "${SCRATCH_BASE%/}/witforge-public-api-policy.XXXXXX")"
trap 'rm -rf "${SCRATCH}"' EXIT

head="$(git -C "${ROOT}" rev-parse HEAD 2>/dev/null || true)"
tag_commit="$(git -C "${ROOT}" rev-parse --verify --quiet "refs/tags/${TAG}^{commit}" || true)"
if [[ -n "${head}" && -n "${tag_commit}" && "${head}" != "${tag_commit}" ]] \
    && git -C "${ROOT}" merge-base --is-ancestor "${tag_commit}" "${head}"; then
    # Protect every item from the newest published release tag at or after TAG (excluding any tag
    # pointing at HEAD itself), so additive items from 0.8.0+ are automatically protected once
    # tagged rather than only freezing v0.7.0.
    baseline_tag="${TAG}"
    while IFS= read -r candidate; do
        [[ "${candidate}" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]] || continue
        cand_commit="$(git -C "${ROOT}" rev-parse --verify --quiet "refs/tags/${candidate}^{commit}" || true)"
        [[ -n "${cand_commit}" && "${cand_commit}" != "${head}" ]] || continue
        if git -C "${ROOT}" merge-base --is-ancestor "${tag_commit}" "${cand_commit}" \
            && git -C "${ROOT}" cat-file -e "${candidate}:docs/design/public-api-inventory.txt" 2>/dev/null; then
            baseline_tag="${candidate}"
            break
        fi
    done <<<"$(git -C "${ROOT}" tag --merged "${head}" --list 'v*' --sort=-v:refname)"
    tagged="${SCRATCH}/tagged.txt"
    git -C "${ROOT}" show \
        "${baseline_tag}:docs/design/public-api-inventory.txt" >"${tagged}"
    removed="${SCRATCH}/removed.txt"
    LC_ALL=C comm -23 "${tagged}" "${ACTUAL}" >"${removed}"
    if [[ -s "${removed}" ]]; then
        echo "the public API removed or changed entries from ${baseline_tag}:" >&2
        cat "${removed}" >&2
        exit 1
    fi
    echo "public API retains every ${baseline_tag} inventory entry"
    exit 0
fi

if [[ -z "${tag_commit}" && -f "${ROOT}/Cargo.toml" ]]; then
    workspace_version="$(awk '
        /^\[workspace\.package\]$/ { section = 1; next }
        /^\[/ { section = 0 }
        section && /^version[[:space:]]*=/ {
            value = $0
            sub(/^[^=]*=[[:space:]]*"/, "", value)
            sub(/"[[:space:]]*$/, "", value)
            print value
            exit
        }
    ' "${ROOT}/Cargo.toml" || true)"
    if [[ -n "${workspace_version}" \
        && "${workspace_version}" != "0.6.0" \
        && "${workspace_version}" != "0.7.0" ]]; then
        echo "missing release tag ${TAG} in ${ROOT} (workspace version is ${workspace_version}); run \`git fetch --tags\` so public API compatibility can compare against the tagged baseline" >&2
        exit 1
    fi
fi

baseline="${ROOT}/ci/public-api-baseline-0.6.0.txt"
approved="${ROOT}/ci/public-api-breaking-0.6.0.diff"
actual_break="${SCRATCH}/breaking.diff"
approved_changes="${SCRATCH}/approved-changes.txt"
actual_changes="${SCRATCH}/actual-changes.txt"
status=0
diff -u --label v0.6.0 --label current "${baseline}" "${ACTUAL}" >"${actual_break}" \
    || status=$?
if [[ ${status} -gt 1 ]]; then
    echo "failed to compare the published and current public API inventories" >&2
    exit "${status}"
fi
# Hunk offsets and unchanged context move after additive API growth. Compare the complete ordered
# changed-line inventory; every added or removed API line remains exact and reviewable.
awk '/^[+-]/ && !/^(---|\+\+\+)/ { print }' "${approved}" >"${approved_changes}"
awk '/^[+-]/ && !/^(---|\+\+\+)/ { print }' "${actual_break}" >"${actual_changes}"
if ! diff -u "${approved_changes}" "${actual_changes}"; then
    echo >&2
    echo "the public API delta from 0.6.0 differs from the exact reviewed inventory" >&2
    exit 1
fi
