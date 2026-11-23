#!/usr/bin/env bash
# Exercise the exact pre-0.7 exception and the additive post-tag inventory policy.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SCRATCH_BASE="${TMPDIR:-${HOME}/.tmp}"
mkdir -p "${SCRATCH_BASE}"
TMP="$(mktemp -d "${SCRATCH_BASE%/}/witforge-public-api-policy-test.XXXXXX")"
trap 'rm -rf "${TMP}"' EXIT
repo="${TMP}/repo"
mkdir -p "${repo}/ci" "${repo}/docs/design"
git -C "${repo}" init -q
git -C "${repo}" config user.name witforge-test
git -C "${repo}" config user.email witforge-test@example.invalid
printf 'item a\n' >"${repo}/ci/public-api-baseline-0.6.0.txt"
printf 'item a\nitem b\n' >"${repo}/reviewed.txt"
status=0
diff -u --label v0.6.0 --label current \
    "${repo}/ci/public-api-baseline-0.6.0.txt" "${repo}/reviewed.txt" \
    >"${repo}/ci/public-api-breaking-0.6.0.diff" || status=$?
[[ ${status} -eq 1 ]]
printf 'item a\nitem b\n' >"${repo}/docs/design/public-api-inventory.txt"
git -C "${repo}" add .
git -C "${repo}" commit -qm baseline

policy="${ROOT}/ci/check-public-api-policy.sh"
bash "${policy}" "${repo}" "${repo}/reviewed.txt"
printf 'item a\nitem c\n' >"${repo}/actual.txt"
if bash "${policy}" "${repo}" "${repo}/actual.txt" >/dev/null 2>&1; then
    echo "pre-tag policy accepted an unreviewed delta" >&2
    exit 1
fi

git -C "${repo}" tag -a v0.7.0 -m v0.7.0
# The exact tag commit remains under the reviewed 0.6.0 -> 0.7.0 exception.
bash "${policy}" "${repo}" "${repo}/reviewed.txt"
printf 'post-tag\n' >"${repo}/marker"
git -C "${repo}" add marker
git -C "${repo}" commit -qm post-tag
printf 'item a\nitem b\nitem c\n' >"${repo}/actual.txt"
bash "${policy}" "${repo}" "${repo}/actual.txt"
printf 'item a\n' >"${repo}/actual.txt"
if bash "${policy}" "${repo}" "${repo}/actual.txt" >/dev/null 2>&1; then
    echo "post-tag policy accepted removal of a tagged API entry" >&2
    exit 1
fi

echo "public API release policy tests passed"
