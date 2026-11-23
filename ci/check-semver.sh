#!/usr/bin/env bash
# Enforce ordinary minor compatibility, or the exact reviewed pre-1.0 break before 0.7.0.
set -euo pipefail

ROOT="${1:-.}"
ROOT="$(cd "${ROOT}" && pwd)"
cd "${ROOT}"
RELEASE_TYPE="$(bash "${ROOT}/ci/semver-release-type.sh" "${ROOT}")"
if [[ "${RELEASE_TYPE}" == "minor" ]]; then
  exec env CARGO_TERM_COLOR=never cargo semver-checks check-release -p witforge --release-type minor
fi
if [[ "${RELEASE_TYPE}" != "major" ]]; then
  echo "unexpected semver release type: ${RELEASE_TYPE}" >&2
  exit 1
fi

SCRATCH_BASE="${TMPDIR:-${HOME}/.tmp}"
mkdir -p "${SCRATCH_BASE}"
LOG="$(mktemp "${SCRATCH_BASE%/}/witforge-semver.XXXXXX")"
ACTUAL="$(mktemp "${SCRATCH_BASE%/}/witforge-semver-api.XXXXXX")"
trap 'rm -f "${LOG}" "${ACTUAL}"' EXIT

if CARGO_TERM_COLOR=never cargo semver-checks check-release -p witforge --release-type minor >"${LOG}" 2>&1; then
  echo "the 0.7.0 exception is active, but the reviewed pre-1.0 break was not reported" >&2
  exit 1
fi
cat "${LOG}"

awk '
  /^--- failure / {
    failure = $3
    sub(/:$/, "", failure)
    next
  }
  /^Failed in:$/ { failed = 1; next }
  failed && /^$/ { failed = 0; next }
  failed && /^  / {
    line = $0
    sub(/^  /, "", line)
    if (failure == "enum_marked_non_exhaustive" ||
        failure == "struct_marked_non_exhaustive" ||
        failure == "enum_variant_added") {
      split(line, parts, /[[:space:]]+/)
      print failure " " parts[1] " " parts[2]
    } else if (failure == "function_missing") {
      sub(/, previously.*/, "", line)
      print failure " " line
    } else if (failure == "pub_module_level_const_missing") {
      split(line, parts, /[[:space:]]+/)
      print failure " constant " parts[1]
    } else {
      print failure " " line
    }
  }
' "${LOG}" | sort -u >"${ACTUAL}"

if ! diff -u "${ROOT}/ci/semver-breaking-api.txt" "${ACTUAL}"; then
  echo >&2
  echo "the semver failures differ from the reviewed pre-1.0 exception inventory" >&2
  exit 1
fi

echo "semver: only the reviewed pre-1.0 break is present"
