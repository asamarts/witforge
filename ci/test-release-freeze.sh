#!/usr/bin/env bash
# Tests for ci/check-release-freeze.sh, in a scratch repository: an untouched release passes; a
# released directory changed in a later commit, a removed entry, a deleted manifest, a shallow
# clone, and invalid or duplicate entries fail; a new entry that is not committed yet passes.
set -euo pipefail

SCRIPT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/check-release-freeze.sh"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/witforge-freeze.XXXXXX")"
trap 'rm -rf "${TMP}"' EXIT

failures=0
# expect <name> <0|1> [text the output must contain]: runs the check in the current directory.
expect() {
    local output status=0
    # The bash running these tests runs the check, so `/bin/bash ci/test-release-freeze.sh` on
    # macOS tests it with bash 3.2.
    output="$("${BASH}" "${SCRIPT}" wit 2>&1)" || status=$?
    if [[ "$2" == 0 && ${status} -ne 0 ]] || [[ "$2" == 1 && ${status} -eq 0 ]]; then
        echo "FAIL: $1 (exit ${status}): ${output}" >&2
        failures=$((failures + 1))
    elif [[ -n "${3:-}" ]] && ! grep -qF -- "$3" <<<"${output}"; then
        echo "FAIL: $1: output lacks \"$3\": ${output}" >&2
        failures=$((failures + 1))
    else
        echo "ok: $1"
    fi
}

commit() {
    git add -A
    git commit -qm "$1"
}

# A repository with v0.1 released and v0.2 in development.
fresh() {
    rm -rf "${TMP}/repo"
    git init -q "${TMP}/repo"
    cd "${TMP}/repo"
    git config user.name test
    git config user.email test@example.invalid
    mkdir -p wit/v0.1 wit/v0.2
    for version in v0.1 v0.2; do
        echo "package a:b@${version#v}.0;" >"wit/${version}/a.wit"
        echo "a:b/x = 1 field u32" >"wit/${version}/wire.lock"
    done
    commit "two versions"
    printf '# released versions\nv0.1\n' >wit/released
    commit "release v0.1"
}

fresh
expect "an untouched release passes" 0 "passed"
echo "// more" >>wit/v0.2/a.wit
commit "change the development version"
expect "a change to an unreleased version passes" 0

fresh
echo "// edited" >>wit/v0.1/a.wit
commit "edit a released version"
echo "// later" >>wit/v0.2/a.wit
commit "a later push"
expect "a released directory changed in an earlier commit fails" 1 "changed since its release"

fresh
echo "// uncommitted" >>wit/v0.1/wire.lock
expect "an uncommitted change to a released directory fails" 1 "changed since its release"

fresh
printf '# none\n' >wit/released
commit "unrelease v0.1"
expect "a removed entry fails" 1 "was removed"

fresh
git rm -q wit/released
commit "delete the manifest"
expect "a deleted manifest fails" 1 "was removed"

fresh
printf 'v0.1\nv0.2\n' >wit/released
expect "a new entry not yet committed passes" 0 "passed"
commit "release v0.2"
expect "a new entry once committed passes" 0 "passed"

for entry in "..." ".hidden" "v0 1" "../v0.1" "v0.1/"; do
    fresh
    printf 'v0.1\n%s\n' "${entry}" >wit/released
    expect "the entry \`${entry}\` is rejected" 1 "invalid released entry"
done

fresh
printf 'v0.1\nv0.1\n' >wit/released
expect "a duplicate entry is rejected" 1 "duplicate"

fresh
printf 'v0.1\nv0.3\n' >wit/released
expect "a missing released directory is rejected" 1 "does not exist"

fresh
git clone -q --depth 1 "file://${TMP}/repo" "${TMP}/shallow"
cd "${TMP}/shallow"
expect "a shallow clone is refused" 1 "shallow"

rm -rf "${TMP}/empty"
git init -q "${TMP}/empty"
cd "${TMP}/empty"
mkdir wit
expect "no manifest and no history passes" 0 "nothing to freeze"

if [[ ${failures} -ne 0 ]]; then
    echo "${failures} release-freeze test(s) failed" >&2
    exit 1
fi
echo "release-freeze tests passed"
