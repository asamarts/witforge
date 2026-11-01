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
#   - each listed directory must exist, not be a symlink or hold one, hold a wire.lock that git
#     tracks under exactly the entry's spelling (so a case-insensitive file system cannot match
#     `V0.1` to `v0.1`), have no untracked files (ignored ones included), and be unchanged in
#     the working tree since every commit whose manifest listed it.
# When the WIT root was moved (`git mv`), the manifest's history at its earlier paths is read
# too, as git's rename detection (`git log --follow`) finds them: each directory an earlier
# manifest listed must still be listed and match what that commit held at the earlier path. Only
# renames are followed: `--follow` also reports a copy, a new manifest that resembles one that
# still exists (another WIT root's, say), and the history beyond a copy belongs to that other
# manifest. A move that also rewrites most of the manifest, or leaves a manifest at the earlier
# path, is not detected as one, so move a WIT root in a commit that changes nothing else.
# Paths are given to git literally, never as patterns, and read back unquoted, so a WIT root
# whose path holds `*`, `?`, `[`, or non-ASCII characters is checked as spelled.
# A commit that lists a directory therefore fixes its content for as long as the commit stays
# in the history the check reads, on whichever branch it was made; rewriting history to drop it
# is outside the check, so protect the branch. The check needs the full history (a shallow
# clone fails) and runs with the bash 3.2 and BSD tools of stock macOS.
set -euo pipefail
# ASCII character classes, so `[:space:]` trims what the manifest parser trims.
export LC_ALL=C
# Every pathspec below is a path: a `*`, `?`, or `[` in the WIT root's path must not match other
# paths.
export GIT_LITERAL_PATHSPECS=1

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
        # git compares a symlink by its target string, so content behind a link could change
        # without the diff below seeing it.
        [[ ! -L "${directory}" ]] \
            || fail "released directory \`${directory}\` is a symlink; release the directory itself"
        [[ -d "${directory}" ]] || fail "released directory \`${directory}\` does not exist"
        [[ -f "${directory}/wire.lock" ]] \
            || fail "released directory \`${directory}\` has no wire.lock"
        # Pathspecs are case-sensitive, so on a case-insensitive file system an entry spelled
        # differently from the directory would match no tracked file and check nothing.
        # `-z` prints the path as it is, where git would quote one with unusual characters.
        tracked="$(git ls-files -z -- "${directory}/wire.lock" | tr '\0' '\n')"
        [[ "${tracked}" == "${directory}/wire.lock" ]] \
            || fail "released directory \`${directory}\` has no wire.lock tracked by git under that exact spelling"
        links="$(git ls-files -s -- "${directory}/" | awk '$1 == "120000" { sub(/^[^\t]*\t/, ""); print }')"
        [[ -z "${links}" ]] \
            || fail "released directory \`${directory}\` holds symlinks: ${links//$'\n'/, }"
        untracked="$(git ls-files --others -- "${directory}/")"
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

# Earlier paths of the manifest, from before the WIT root was moved. An entry an earlier manifest
# listed must still be listed, and its directory must hold what that commit held at the earlier
# path: the tree ids are compared, together with the working tree against HEAD.
# `--follow` reports a rename (`R<score>`, the earlier path no longer exists) and a copy
# (`C<score>`, it still does) alike; it lists the status, then the earlier and the later path for
# either, and one path for any other status. Only renames are followed: a copy's earlier path is
# another manifest with its own history, such as a second WIT root's that resembles this one.
earlier=""
if [[ -n "${log}" ]]; then
    follow="$(git log --follow --name-status -z --format= -- "${MANIFEST}" | tr '\0' '\n')"
    while IFS= read -r status; do
        case "${status}" in
            R*)
                IFS= read -r path
                IFS= read -r _
                earlier="${earlier}${path}"$'\n'
                ;;
            C*) break ;;
            ?*) IFS= read -r _ ;;
        esac
    done <<<"${follow}"
    earlier="$(printf '%s' "${earlier}" | sort -u)"
fi
while IFS= read -r path; do
    [[ -n "${path}" ]] || continue
    case "${path}" in
        */released) old_prefix="${path%released}" ;;
        released) old_prefix="" ;;
        *) continue ;;
    esac
    old_log="$(git log --full-history --reverse --format=%H -- "${path}")"
    while IFS= read -r commit; do
        [[ -n "${commit}" ]] || continue
        git cat-file -e "${commit}:${path}" 2>/dev/null || continue
        listed_entries="$(git show "${commit}:${path}" | entries)"
        while IFS= read -r entry; do
            [[ -n "${entry}" ]] || continue
            listed "${entry}" \
                || fail "released entry \`${entry}\` (listed in ${commit:0:12} at ${path}) was removed from ${MANIFEST}"
            released_tree="$(git rev-parse --verify --quiet "${commit}:${old_prefix}${entry}")" \
                || fail "released directory \`${old_prefix}${entry}\` is missing from ${commit:0:12}"
            head_tree="$(git rev-parse --verify --quiet "HEAD:${PREFIX}${entry}")" \
                || fail "released directory \`${PREFIX}${entry}\` is not committed"
            if [[ "${released_tree}" != "${head_tree}" ]] \
                || ! git diff --quiet HEAD -- "${PREFIX}${entry}"; then
                fail "released directory \`${PREFIX}${entry}\` changed since its release as \`${old_prefix}${entry}\` in ${commit:0:12}"
            fi
            checked=$((checked + 1))
        done <<<"${listed_entries}"
    done <<<"${old_log}"
done <<<"${earlier}"

echo "release freeze check passed for ${ROOT} (${checked} release check(s))"
