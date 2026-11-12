#!/usr/bin/env bash
# Generate every emitter into a temporary tree, verify that `--check` reproduces it, and check
# the committed runnable-example outputs. Generation must succeed and remain deterministic.
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
"${RUN[@]}" schema "${WIT_ROOT}" --out "${TMP}/schema"
"${RUN[@]}" glue "${WIT_ROOT}" --out "${TMP}/glue" \
    --plain-root crate::plain --proto-root crate::proto

"${RUN[@]}" rust "${WIT_ROOT}" --out "${TMP}/plain" --check
"${RUN[@]}" proto "${WIT_ROOT}" --out "${TMP}/proto" --check
"${RUN[@]}" schema "${WIT_ROOT}" --out "${TMP}/schema" --check
"${RUN[@]}" glue "${WIT_ROOT}" --out "${TMP}/glue" \
    --plain-root crate::plain --proto-root crate::proto --check

for example in single evolved; do
    config="${ROOT}/examples/${example}/witforge.toml"
    for emitter in rust proto schema glue; do
        "${RUN[@]}" "${emitter}" --config "${config}" --check
    done
done

echo "committed example output check passed"
echo "generated output check passed for ${WIT_ROOT}"
