#!/usr/bin/env bash
# Generate every emitter into a temporary tree, verify that `--check` reproduces it, and check
# the committed runnable-example outputs. Generation must succeed and remain deterministic.
#
# Usage: [WITFORGE=path/to/witforge] ci/check-generated.sh [WIT_ROOT]
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WIT_ROOT="${1:-${ROOT}/crates/witforge/tests/fixtures/versions/wit}"
scratch="${TMPDIR:-${HOME}/.tmp}"
mkdir -p "${scratch}"
TMP="$(mktemp -d "${scratch}/witforge-generated.XXXXXX")"
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
"${RUN[@]}" doc "${WIT_ROOT}" --out "${TMP}/doc"
"${RUN[@]}" ts "${WIT_ROOT}" --out "${TMP}/ts"
"${RUN[@]}" ts-glue "${WIT_ROOT}" --out "${TMP}/ts-glue" \
    --plain-root ../ts --proto-root ../proto
"${RUN[@]}" py "${WIT_ROOT}" --out "${TMP}/py"
"${RUN[@]}" py-glue "${WIT_ROOT}" --out "${TMP}/py-glue" \
    --plain-root ..py --proto-root ..proto
"${RUN[@]}" conformance --out "${TMP}/conformance"
"${RUN[@]}" conformance --out "${TMP}/conformance-poly" --polyglot-proto-options

"${RUN[@]}" rust "${WIT_ROOT}" --out "${TMP}/plain" --check
"${RUN[@]}" proto "${WIT_ROOT}" --out "${TMP}/proto" --check
"${RUN[@]}" schema "${WIT_ROOT}" --out "${TMP}/schema" --check
"${RUN[@]}" glue "${WIT_ROOT}" --out "${TMP}/glue" \
    --plain-root crate::plain --proto-root crate::proto --check
"${RUN[@]}" doc "${WIT_ROOT}" --out "${TMP}/doc" --check
"${RUN[@]}" ts "${WIT_ROOT}" --out "${TMP}/ts" --check
"${RUN[@]}" ts-glue "${WIT_ROOT}" --out "${TMP}/ts-glue" \
    --plain-root ../ts --proto-root ../proto --check
"${RUN[@]}" py "${WIT_ROOT}" --out "${TMP}/py" --check
"${RUN[@]}" py-glue "${WIT_ROOT}" --out "${TMP}/py-glue" \
    --plain-root ..py --proto-root ..proto --check
"${RUN[@]}" conformance --out "${TMP}/conformance" --check
"${RUN[@]}" conformance --out "${TMP}/conformance-poly" --polyglot-proto-options --check

for example in single evolved; do
    config="${ROOT}/examples/${example}/witforge.toml"
    for emitter in rust proto schema glue; do
        "${RUN[@]}" "${emitter}" --config "${config}" --check
    done
done

# Compile and round-trip the committed generated output of every example in Editions 2021 and 2024
# against prost and pbjson with rustfmt, Clippy -D warnings, -D missing_docs, and cargo doc.
cargo test --manifest-path "${ROOT}/Cargo.toml" -p witforge --test compile_matrix --locked -- \
    examples_generated_code_compiles_and_roundtrips

echo "committed example output check and compile/round-trip passed"
echo "generated output check passed for ${WIT_ROOT}"
