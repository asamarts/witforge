#!/usr/bin/env bash
# Package a built witforge binary and its checksum for a GitHub release.
#
# Usage: ci/release-asset.sh VERSION TARGET BINARY [OUT_DIR]
# Version may start with `v`; it is stripped from the asset name. Prints the asset path.
set -euo pipefail

if [ "$#" -lt 3 ] || [ "$#" -gt 4 ]; then
  echo "usage: $0 VERSION TARGET BINARY [OUT_DIR]" >&2
  exit 2
fi

version=${1#v}
target=$2
binary=$3
out=${4:-dist}

if [ ! -f "$binary" ]; then
  echo "binary not found: $binary" >&2
  exit 1
fi
if ! [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "invalid version: $version (expected X.Y.Z)" >&2
  exit 1
fi

sha256() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{print $1}'
  else
    openssl dgst -sha256 "$1" | awk '{print $NF}'
  fi
}

mkdir -p "$out"
staging=$(mktemp -d)
trap 'rm -rf "$staging"' EXIT
cp "$binary" "$staging/"
name="witforge-${version}-${target}.tar.gz"
archive="$out/$name"
tar -czf "$archive" -C "$staging" "$(basename "$binary")"

digest=$(sha256 "$archive")
printf '%s  %s\n' "$digest" "$name" >"${archive}.sha256"
if [ "$(sha256 "$archive")" != "$digest" ]; then
  echo "checksum self-check failed for $archive" >&2
  exit 1
fi
printf '%s\n' "$archive"
