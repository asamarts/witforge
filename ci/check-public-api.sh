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
