#!/usr/bin/env bash
# Thin wrapper invoking `witforge freeze` so existing workflows and scripts that call
# `ci/check-release-freeze.sh [WIT_ROOT]` continue to work unchanged.
#
# Usage: ci/check-release-freeze.sh [WIT_ROOT] [EXTRA_FREEZE_ARGS...]
#
# Without arguments, checks the versions fixture exercised by witforge's CI. Set `WITFORGE` to
# select a specific binary; otherwise uses the workspace build (`target/debug/witforge` or
# `target/release/witforge`) or `witforge` on PATH.
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
repo_root="$(cd "${script_dir}/.." && pwd -P)"

witforge_bin="${WITFORGE:-}"
if [[ -z "${witforge_bin}" ]]; then
    if [[ -x "${repo_root}/target/debug/witforge" ]]; then
        witforge_bin="${repo_root}/target/debug/witforge"
    elif [[ -x "${repo_root}/target/release/witforge" ]]; then
        witforge_bin="${repo_root}/target/release/witforge"
    elif command -v witforge >/dev/null 2>&1; then
        witforge_bin="$(command -v witforge)"
    else
        echo "release freeze: witforge binary not found; set WITFORGE or run \`cargo build -p witforge-cli\`" >&2
        exit 2
    fi
fi

if [[ $# -eq 0 ]]; then
    exec "${witforge_bin}" freeze "${repo_root}/crates/witforge/tests/fixtures/versions/wit"
fi

exec "${witforge_bin}" freeze "$@"
