#!/usr/bin/env bash
# Fail if a released version directory or an existing released-manifest entry changed.
#
# Usage: ci/check-release-freeze.sh [WIT_ROOT] [BASE_COMMIT]
#
# WIT_ROOT defaults to the versions fixture exercised by CI. BASE_COMMIT defaults to HEAD, which
# makes a local run a manifest-validation check; CI passes the merge-base/push predecessor.
set -euo pipefail

REPO="$(git rev-parse --show-toplevel)"
ROOT="${1:-crates/witforge/tests/fixtures/versions/wit}"
BASE="${2:-HEAD}"
if [[ "${ROOT}" = /* ]]; then
    ROOT="$(realpath --relative-to="${REPO}" "${ROOT}")"
else
    ROOT="${ROOT#./}"
fi
MANIFEST="${ROOT}/released"

if [[ ! -f "${MANIFEST}" ]]; then
    echo "no released manifest at ${MANIFEST}; nothing to freeze"
    exit 0
fi

entries() {
    local file="$1"
    local line
    while IFS= read -r line || [[ -n "${line}" ]]; do
        line="${line%$'\r'}"
        line="${line#"${line%%[![:space:]]*}"}"
        line="${line%"${line##*[![:space:]]}"}"
        [[ -z "${line}" || "${line}" == \#* ]] && continue
        printf '%s\n' "${line}"
    done < "${file}"
}

declare -A seen=()
while IFS= read -r entry; do
    [[ "${entry}" =~ ^[A-Za-z0-9._-]+$ ]] || {
        echo "invalid released entry \`${entry}\` in ${MANIFEST}" >&2
        exit 1
    }
    [[ "${entry}" == "." || "${entry}" == *..* ]] && {
        echo "released entry \`${entry}\` is not a simple directory name" >&2
        exit 1
    }
    [[ -z "${seen[${entry}]:-}" ]] || {
        echo "duplicate released entry \`${entry}\`" >&2
        exit 1
    }
    seen["${entry}"]=1
    [[ -d "${REPO}/${ROOT}/${entry}" ]] || {
        echo "released directory \`${ROOT}/${entry}\` does not exist" >&2
        exit 1
    }
    [[ -f "${REPO}/${ROOT}/${entry}/wire.lock" ]] || {
        echo "released directory \`${ROOT}/${entry}\` has no wire.lock" >&2
        exit 1
    }
done < <(entries "${MANIFEST}")

git -C "${REPO}" rev-parse --verify "${BASE}^{commit}" >/dev/null 2>&1 || {
    echo "base commit \`${BASE}\` is not resolvable" >&2
    exit 1
}

if git -C "${REPO}" cat-file -e "${BASE}:${MANIFEST}" 2>/dev/null; then
    declare -A old=()
    while IFS= read -r entry; do
        old["${entry}"]=1
        [[ -n "${seen[${entry}]:-}" ]] || {
            echo "released entry \`${entry}\` was removed from ${MANIFEST}" >&2
            exit 1
        }
    done < <(git -C "${REPO}" show "${BASE}:${MANIFEST}" | entries /dev/stdin)
    for entry in "${!old[@]}"; do
        if ! git -C "${REPO}" diff --quiet "${BASE}" -- "${ROOT}/${entry}"; then
            echo "released directory \`${ROOT}/${entry}\` changed against ${BASE}" >&2
            exit 1
        fi
    done
fi

echo "release freeze check passed for ${ROOT}"
