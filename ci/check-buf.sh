#!/usr/bin/env bash
# Optional protobuf compatibility check. Not part of the default gate: the lockfile is the
# normative wire check, and `buf breaking` is an additional consumer-side safeguard.
#
# Generates the protobuf schema from WIT_ROOT as it was at BASE and as it is in the working
# tree, then runs `buf breaking` on the second against the first. Without `buf` it skips, unless
# BUF_REQUIRED=1 (set by the CI job), where it fails.
#
# Usage: [WITFORGE=path/to/witforge] [BUF=path/to/buf] ci/check-buf.sh [WIT_ROOT] [BASE]
#   WIT_ROOT  a WIT root relative to the repository root (default: the versions fixture)
#   BASE      the commit to compare against (default: HEAD^)
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WIT_ROOT="${1:-crates/witforge/tests/fixtures/versions/wit}"
BASE="${2:-HEAD^}"
BUF="${BUF:-buf}"

if ! command -v "${BUF}" >/dev/null 2>&1; then
    if [[ "${BUF_REQUIRED:-}" == 1 ]]; then
        echo "buf is not installed (BUF=${BUF})" >&2
        exit 1
    fi
    echo "buf is not installed; skipping optional protobuf breaking check (BUF_REQUIRED=1 fails)"
    exit 0
fi

cd "${ROOT}"
base_commit="$(git rev-parse --verify "${BASE}^{commit}")"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/witforge-buf.XXXXXX")"
trap 'rm -rf "${TMP}"' EXIT
if [[ -n "${WITFORGE:-}" ]]; then
    RUN=("${WITFORGE}")
else
    RUN=(cargo run --quiet --manifest-path "${ROOT}/crates/witforge-cli/Cargo.toml" --)
fi

if ! git cat-file -e "${base_commit}:${WIT_ROOT}" 2>/dev/null; then
    echo "${WIT_ROOT} does not exist at ${BASE}; nothing to compare"
    exit 0
fi
mkdir -p "${TMP}/base"
git archive "${base_commit}" -- "${WIT_ROOT}" | tar -x -C "${TMP}/base"
"${RUN[@]}" proto "${TMP}/base/${WIT_ROOT}" --out "${TMP}/old"
"${RUN[@]}" proto "${WIT_ROOT}" --out "${TMP}/new"
"${BUF}" breaking "${TMP}/new" --against "${TMP}/old"
echo "buf breaking: no breaking change in ${WIT_ROOT} since ${BASE} (${base_commit:0:12})"
