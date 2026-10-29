#!/usr/bin/env bash
# Optional protobuf compatibility check. Not part of the default gate: the lockfile is the
# normative wire check, and `buf` is an additional consumer-side safeguard.
#
# Usage: [WITFORGE=path/to/witforge] ci/check-buf.sh [WIT_ROOT] [BUF_AGAINST]
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WIT_ROOT="${1:-${ROOT}/crates/witforge/tests/fixtures/versions/wit}"
AGAINST="${2:-.git#branch=main}"

if ! command -v buf >/dev/null 2>&1; then
    echo "buf is not installed; skipping optional protobuf breaking check"
    exit 0
fi

TMP="$(mktemp -d "${TMPDIR:-/tmp}/witforge-buf.XXXXXX")"
trap 'rm -rf "${TMP}"' EXIT
if [[ -n "${WITFORGE:-}" ]]; then
    RUN=("${WITFORGE}")
else
    RUN=(cargo run --quiet --manifest-path "${ROOT}/crates/witforge-cli/Cargo.toml" --)
fi
"${RUN[@]}" proto "${WIT_ROOT}" --out "${TMP}/proto"
buf breaking "${TMP}/proto" --against "${AGAINST}"
