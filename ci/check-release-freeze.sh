#!/usr/bin/env bash
# Fail if a released version directory changed after its release, or a released-manifest entry
# was removed.
#
# Usage: ci/check-release-freeze.sh [WIT_ROOT]
#
# WIT_ROOT is resolved from the current directory; without it the check covers the versions
# fixture exercised by witforge's CI. The check reads the full history of WIT_ROOT/released
# (every commit that changed it, merges included) rather than comparing with one base commit,
# so a change that landed in a push whose CI run was cancelled or failed is still caught by the
# next run:
#   - every entry any committed version of the manifest listed must still be listed (deleting
#     the manifest counts as removing every entry);
#   - each listed directory must exist, hold a wire.lock, have no untracked files, and be
#     unchanged in the working tree since every commit whose manifest listed it.
# A commit that lists a directory therefore fixes its content for good, on whichever branch it
# was made. The check needs the full history (a shallow clone fails) and runs with the bash 3.2
# and BSD tools of stock macOS.
set -euo pipefail
# ASCII character classes, so `[:space:]` trims what the manifest parser trims.
export LC_ALL=C

fail() {
    echo "release freeze: $*" >&2
    exit 1
}

if [[ $# -gt 0 ]]; then
    [[ -d "$1" ]] || fail "WIT root $1 is not a directory"
    absolute="$(cd "$1" && pwd -P)"
else
    script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
    absolute="$(cd "${script_dir}/../crates/witforge/tests/fixtures/versions/wit" && pwd -P)"
fi
top="$(git -C "${absolute}" rev-parse --show-toplevel 2>/dev/null)" \
    || fail "${absolute} is not inside a git repository"
REPO="$(cd "${top}" && pwd -P)"
# Paths below are relative to the repository, as git names them: PREFIX is empty for a WIT root
# at the top of the repository and `<root>/` otherwise.
if [[ "${absolute}" == "${REPO}" ]]; then
    ROOT="."
    PREFIX=""
else
    case "${absolute}/" in
        "${REPO}/"*) ROOT="${absolute#"${REPO}/"}" ;;
        *) fail "${absolute} is not inside the repository ${REPO}" ;;
    esac
    PREFIX="${ROOT}/"
fi
cd "${REPO}"
MANIFEST="${PREFIX}released"

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

shallow="$(git rev-parse --is-shallow-repository)"
if [[ "${shallow}" == true ]]; then
    fail "the repository is a shallow clone; the check needs the manifest's full history"
fi

# Every commit that changed the manifest, merges and side branches included, oldest first.
# `--full-history` keeps a commit that the default simplification would drop because another
# branch made the same change.
log=""
if git rev-parse --verify --quiet HEAD >/dev/null; then
    log="$(git log --full-history --reverse --format=%H -- "${MANIFEST}")"
fi

current=""
if [[ -f "${MANIFEST}" ]]; then
    current="$(entries <"${MANIFEST}")"
elif [[ -z "${log}" ]]; then
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
        directory="${PREFIX}${entry}"
        [[ -d "${directory}" ]] || fail "released directory \`${directory}\` does not exist"
        [[ -f "${directory}/wire.lock" ]] \
            || fail "released directory \`${directory}\` has no wire.lock"
        untracked="$(git ls-files --others --exclude-standard -- "${directory}")"
        [[ -z "${untracked}" ]] \
            || fail "released directory \`${directory}\` has untracked files: ${untracked//$'\n'/, }"
    done <<<"${current}"
fi

# Walk the manifest's history: an entry once listed must still be listed, and its directory must
# match every commit that listed it.
checked=0
while IFS= read -r commit; do
    [[ -n "${commit}" ]] || continue
    # The commit that deleted the manifest has no manifest to read.
    git cat-file -e "${commit}:${MANIFEST}" 2>/dev/null || continue
    manifest="$(git show "${commit}:${MANIFEST}")"
    listed_entries="$(entries <<<"${manifest}")"
    while IFS= read -r entry; do
        [[ -n "${entry}" ]] || continue
        listed "${entry}" \
            || fail "released entry \`${entry}\` (listed in ${commit:0:12}) was removed from ${MANIFEST}"
        git diff --quiet "${commit}" -- "${PREFIX}${entry}" \
            || fail "released directory \`${PREFIX}${entry}\` changed since its release in ${commit:0:12}"
        checked=$((checked + 1))
    done <<<"${listed_entries}"
done <<<"${log}"

echo "release freeze check passed for ${ROOT} (${checked} release check(s))"
