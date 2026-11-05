#!/usr/bin/env bash
# Test release packaging and checksums without a real Rust build.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
printf '#!/bin/sh\necho fake witforge\n' >"$tmp/witforge"
chmod +x "$tmp/witforge"

asset=$(bash "$root/ci/release-asset.sh" v0.3.0 x86_64-unknown-linux-gnu "$tmp/witforge" "$tmp/dist")
[ "$asset" = "$tmp/dist/witforge-0.3.0-x86_64-unknown-linux-gnu.tar.gz" ] || {
  echo "unexpected asset path: $asset" >&2
  exit 1
}
[ -f "$asset" ] || { echo "missing archive: $asset" >&2; exit 1; }
[ -f "${asset}.sha256" ] || { echo "missing checksum" >&2; exit 1; }
tar -tzf "$asset" | grep -qx witforge
expected=$(awk '{print $1}' "${asset}.sha256")
if command -v sha256sum >/dev/null 2>&1; then
  actual=$(sha256sum "$asset" | awk '{print $1}')
else
  actual=$(shasum -a 256 "$asset" | awk '{print $1}')
fi
[ "$expected" = "$actual" ] || { echo "checksum mismatch" >&2; exit 1; }
printf 'release asset test passed: %s\n' "$(basename "$asset")"
