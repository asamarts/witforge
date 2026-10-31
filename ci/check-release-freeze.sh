#!/usr/bin/env bash
# Fail if a released version directory changed after its release, or a released-manifest entry
# was removed.
#
# Usage: ci/check-release-freeze.sh [WIT_ROOT]
#
# WIT_ROOT defaults to the versions fixture exercised by CI. The check reads the history of
# WIT_ROOT/released rather than comparing with one base commit, so a change that landed in a push
# whose CI run was cancelled or failed is still caught by the next run:
#   - every entry any committed version of the manifest listed must still be listed (deleting
#     the manifest counts as removing every entry);
#   - each listed directory must exist, hold a wire.lock, and be unchanged in the working tree
#     since the first commit whose manifest listed it.
# It needs the full history (a shallow clone fails) and runs with the bash 3.2 and BSD tools of
# stock macOS.
set -euo pipefail

REPO="$(cd "$(git rev-parse --show-toplevel)" && pwd -P)"
ROOT="${1:-crates/witforge/tests/fixtures/versions/wit}"
if [[ "${ROOT}" = /* ]]; then
    absolute="$(cd "${ROOT}" && pwd -P)"
    case "${absolute}/" in
        "${REPO}/"*) ROOT="${absolute#"${REPO}/"}" ;;
        *) echo "${ROOT} is not inside the repository ${REPO}" >&2; exit 1 ;;
    esac
else
    ROOT="${ROOT#./}"
fi
ROOT="${ROOT%/}"
cd "${REPO}"
MANIFEST="${ROOT}/released"

fail() {
    echo "release freeze: $*" >&2
    exit 1
}

# The entries of a manifest on stdin, one per line: comments, blank lines, and surrounding
# whitespace removed.
entries() {
    local line
    while IFS= read -r line || [[ -n "${line}" ]]; do
        line="${line%$'\r'}"
        line="${line#"${line%%[![:space:]]*}"}"
        line="${line%"${line##*[![:space:]]}"}"
        [[ -z "${line}" || "${line}" == \#* ]] && continue
        printf '%s\n' "${line}"
    done
}

if [[ "$(git rev-parse --is-shallow-repository)" == true ]]; then
    fail "the repository is a shallow clone; the check needs the manifest's full history"
fi

# The commits that changed the manifest, oldest first.
history=()
while IFS= read -r commit; do
    history+=("${commit}")
done < <(git log --reverse --format=%H -- "${MANIFEST}")

current=""
if [[ -f "${MANIFEST}" ]]; then
    current="$(entries <"${MANIFEST}")"
elif [[ ${#history[@]} -eq 0 ]]; then
    echo "no released manifest at ${MANIFEST}; nothing to freeze"
    exit 0
fi

listed() {
    [[ -n "${current}" ]] && grep -qxF -- "$1" <<<"${current}"
}

seen=""
if [[ -n "${current}" ]]; then
    while IFS= read -r entry; do
        # The same rule as the manifest parser in crates/witforge/src/lib.rs.
        [[ "${entry}" =~ ^[A-Za-z0-9][A-Za-z0-9._-]*$ ]] \
            || fail "invalid released entry \`${entry}\` in ${MANIFEST}: expected a plain directory name"
        if [[ -n "${seen}" ]] && grep -qxF -- "${entry}" <<<"${seen}"; then
            fail "duplicate released entry \`${entry}\` in ${MANIFEST}"
        fi
        seen="${seen}${entry}"$'\n'
        [[ -d "${ROOT}/${entry}" ]] || fail "released directory \`${ROOT}/${entry}\` does not exist"
        [[ -f "${ROOT}/${entry}/wire.lock" ]] \
            || fail "released directory \`${ROOT}/${entry}\` has no wire.lock"
    done <<<"${current}"
fi

# Walk the manifest's history: an entry once listed must still be listed, and its directory must
# be unchanged since the first commit that listed it.
released_at=""
for commit in ${history[@]+"${history[@]}"}; do
    git cat-file -e "${commit}:${MANIFEST}" 2>/dev/null || continue
    while IFS= read -r entry; do
        [[ -n "${entry}" ]] || continue
        if [[ -n "${released_at}" ]] && cut -d ' ' -f 1 <<<"${released_at}" | grep -qxF -- "${entry}"; then
            continue
        fi
        released_at="${released_at}${entry} ${commit}"$'\n'
        listed "${entry}" || fail "released entry \`${entry}\` (listed since ${commit:0:12}) was removed from ${MANIFEST}"
        git diff --quiet "${commit}" -- "${ROOT}/${entry}" \
            || fail "released directory \`${ROOT}/${entry}\` changed since its release in ${commit:0:12}"
    done < <(git show "${commit}:${MANIFEST}" | entries)
done

echo "release freeze check passed for ${ROOT}"
