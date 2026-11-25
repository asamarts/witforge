#!/usr/bin/env bash
# Prove that the 0.7.0 semver exception applies before/tag-time and expires after the tag.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SCRATCH_BASE="${TMPDIR:-${HOME}/.tmp}"
mkdir -p "${SCRATCH_BASE}"
REPO="$(mktemp -d "${SCRATCH_BASE%/}/witforge-semver-type.XXXXXX")"
trap 'rm -rf "${REPO}"' EXIT
mkdir -p "${REPO}/ci"
cp "${ROOT}/ci/semver-breaking-release" "${REPO}/ci/"
git -C "${REPO}" init -q
git -C "${REPO}" config user.name witforge-test
git -C "${REPO}" config user.email witforge-test@example.invalid

write_version() {
  cat >"${REPO}/Cargo.toml" <<EOF
[workspace]
members = []

[workspace.package]
version = "$1"
EOF
}

expect_type() {
  local expected="$1"
  local actual
  actual="$(bash "${ROOT}/ci/semver-release-type.sh" "${REPO}")"
  if [[ "${actual}" != "${expected}" ]]; then
    echo "expected release type ${expected}, got ${actual}" >&2
    exit 1
  fi
}

write_version 0.6.0
git -C "${REPO}" add Cargo.toml ci/semver-breaking-release
git -C "${REPO}" commit -qm base
BASE_SHA="$(git -C "${REPO}" rev-parse HEAD)"
expect_type major

write_version 0.7.0
git -C "${REPO}" add Cargo.toml
git -C "${REPO}" commit -qm release-preparation
expect_type major
git -C "${REPO}" tag v0.7.0
expect_type major
# Checking out a pre-tag commit after v0.7.0 exists must still return major (not minor).
git -C "${REPO}" checkout -q "${BASE_SHA}"
expect_type major
git -C "${REPO}" checkout -q -

echo post-release >"${REPO}/post-release"
git -C "${REPO}" add post-release
git -C "${REPO}" commit -qm post-release
expect_type minor

write_version 0.8.0
git -C "${REPO}" add Cargo.toml
git -C "${REPO}" commit -qm next-minor
expect_type minor
