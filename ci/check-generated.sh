#!/usr/bin/env bash
# Generate every emitter into a temporary tree and verify it with --check.
#
# Usage: [WITFORGE=path/to/witforge] ci/check-generated.sh [WIT_ROOT]
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WIT_ROOT="${1:-${ROOT}/crates/witforge/tests/fixtures/versions/wit}"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/witforge-generated.XXXXXX")"
trap 'rm -rf "${TMP}"' EXIT

if [[ -n "${WITFORGE:-}" ]]; then
    RUN=("${WITFORGE}")
else
    RUN=(cargo run --quiet --manifest-path "${ROOT}/crates/witforge-cli/Cargo.toml" --)
fi

"${RUN[@]}" rust "${WIT_ROOT}" --out "${TMP}/plain"
"${RUN[@]}" proto "${WIT_ROOT}" --out "${TMP}/proto"
"${RUN[@]}" glue "${WIT_ROOT}" --out "${TMP}/glue" \
    --plain-root crate::plain --proto-root crate::proto

"${RUN[@]}" rust "${WIT_ROOT}" --out "${TMP}/plain" --check
"${RUN[@]}" proto "${WIT_ROOT}" --out "${TMP}/proto" --check
"${RUN[@]}" glue "${WIT_ROOT}" --out "${TMP}/glue" \
    --plain-root crate::plain --proto-root crate::proto --check

echo "generated output check passed for ${WIT_ROOT}"
