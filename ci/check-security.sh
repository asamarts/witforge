#!/usr/bin/env bash
# Dependency advisory and license/source policy gates.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

cargo audit
cargo deny check --config ci/deny.toml advisories bans licenses sources
