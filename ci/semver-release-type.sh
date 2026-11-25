#!/usr/bin/env bash
# Select cargo-semver-checks' release type. The only intentional pre-1.0 API break is 0.7.0.
set -euo pipefail

ROOT="${1:-.}"
ROOT="$(cd "${ROOT}" && pwd)"
EXCEPTION_FILE="${ROOT}/ci/semver-breaking-release"
read -r BASE_VERSION BREAKING_VERSION EXTRA <"${EXCEPTION_FILE}"
if [[ -n "${EXTRA:-}" \
  || ! "${BASE_VERSION}" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ \
  || ! "${BREAKING_VERSION}" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "invalid semver exception in ${EXCEPTION_FILE}" >&2
  exit 1
fi

WORKSPACE_VERSION="$({
  awk '
    /^\[workspace\.package\]$/ { section = 1; next }
    /^\[/ { section = 0 }
    section && /^version[[:space:]]*=/ {
      value = $0
      sub(/^[^=]*=[[:space:]]*"/, "", value)
      sub(/"[[:space:]]*$/, "", value)
      print value
      exit
    }
  ' "${ROOT}/Cargo.toml"
} || true)"
if [[ ! "${WORKSPACE_VERSION}" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "could not read workspace version from ${ROOT}/Cargo.toml" >&2
  exit 1
fi

# Before 0.7.0 is published, both the 0.6.0 development tree and the 0.7.0 release-preparation
# tree contain the reviewed break. The tag workflow also checks the tag commit itself. Once the
# tag is on an ancestor of HEAD, normal minor compatibility is restored without a cleanup edit.
TAG_COMMIT="$(git -C "${ROOT}" rev-list -n 1 "refs/tags/v${BREAKING_VERSION}" 2>/dev/null || true)"
HEAD_COMMIT="$(git -C "${ROOT}" rev-parse HEAD 2>/dev/null || true)"
if [[ -n "${TAG_COMMIT}" && -n "${HEAD_COMMIT}" && "${TAG_COMMIT}" != "${HEAD_COMMIT}" ]] \
  && git -C "${ROOT}" merge-base --is-ancestor "${TAG_COMMIT}" "${HEAD_COMMIT}"; then
  echo minor
elif [[ "${WORKSPACE_VERSION}" == "${BASE_VERSION}" \
  || "${WORKSPACE_VERSION}" == "${BREAKING_VERSION}" ]]; then
  echo major
else
  echo minor
fi
