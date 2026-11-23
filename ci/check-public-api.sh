#!/usr/bin/env bash
# Compare the documented 1.0 Rust library API with rustdoc's public item graph.
set -euo pipefail

ROOT="${1:-.}"
ROOT="$(cd "${ROOT}" && pwd)"
TOOLCHAIN="${PUBLIC_API_TOOLCHAIN:-nightly-2026-09-11}"
SCRATCH_BASE="${TMPDIR:-${HOME}/.tmp}"
mkdir -p "${SCRATCH_BASE}"
SCRATCH="$(mktemp -d "${SCRATCH_BASE%/}/witforge-public-api.XXXXXX")"
trap 'rm -rf "${SCRATCH}"' EXIT

CARGO_TARGET_DIR="${SCRATCH}/target" \
  cargo "+${TOOLCHAIN}" rustdoc \
  --manifest-path "${ROOT}/Cargo.toml" \
  -p witforge --lib --locked -- \
  -Z unstable-options --output-format json >/dev/null

JSON="${SCRATCH}/target/doc/witforge.json"
ACTUAL="${SCRATCH}/actual.txt"
EXPECTED="${ROOT}/docs/design/public-api-inventory.txt"
jq -r -f "${ROOT}/ci/public-api-inventory.jq" "${JSON}" >"${ACTUAL}"

if ! diff -u "${EXPECTED}" "${ACTUAL}"; then
  echo >&2
  echo "docs/design/public-api-inventory.txt does not match the public Rust API." >&2
  echo "Review the API change, then replace that file with:" >&2
  cat "${ACTUAL}" >&2
  exit 1
fi

BASELINE="${ROOT}/ci/public-api-baseline-0.6.0.txt"
APPROVED_BREAK="${ROOT}/ci/public-api-breaking-0.6.0.diff"
ACTUAL_BREAK="${SCRATCH}/breaking.diff"
APPROVED_CHANGES="${SCRATCH}/approved-changes.txt"
ACTUAL_CHANGES="${SCRATCH}/actual-changes.txt"
status=0
diff -u --label v0.6.0 --label current "${BASELINE}" "${ACTUAL}" >"${ACTUAL_BREAK}" \
  || status=$?
if [[ ${status} -gt 1 ]]; then
  echo "failed to compare the published and current public API inventories" >&2
  exit "${status}"
fi
# Hunk offsets and unchanged context move after additive API growth. Compare the complete ordered
# changed-line inventory; every added or removed API line remains exact and reviewable.
grep -E '^[+-]' "${APPROVED_BREAK}" | grep -Ev '^(---|\+\+\+)' >"${APPROVED_CHANGES}"
grep -E '^[+-]' "${ACTUAL_BREAK}" | grep -Ev '^(---|\+\+\+)' >"${ACTUAL_CHANGES}"
if ! diff -u "${APPROVED_CHANGES}" "${ACTUAL_CHANGES}"; then
  echo >&2
  echo "the public API delta from 0.6.0 differs from the exact reviewed inventory" >&2
  exit 1
fi
