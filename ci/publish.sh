#!/usr/bin/env bash
# Publish the workspace crates whose version is not on crates.io yet, library first.
#
# Usage: ci/publish.sh [--check | --dry-run]
#   (none)     publish the missing crates. Credentials come from CARGO_REGISTRY_TOKEN (the
#              release workflow mints a short-lived one with trusted publishing) or from
#              `cargo login`.
#   --check    print the crates still to publish, one per line, and publish nothing
#   --dry-run  package and verify the missing crates without uploading
#
# A crate already on crates.io at its version is skipped, so a run that failed halfway can be
# repeated.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${ROOT}"
# Dependency order: the CLI depends on the library through the registry.
CRATES=(witforge witforge-cli)

mode=publish
case "${1:-}" in
    "") ;;
    --check) mode=check ;;
    --dry-run) mode=dry-run ;;
    *) echo "usage: $0 [--check | --dry-run]" >&2; exit 2 ;;
esac

metadata="$(cargo metadata --no-deps --format-version 1 --locked)"
version_of() {
    jq -r --arg name "$1" '.packages[] | select(.name == $name) | .version' <<<"${metadata}"
}

# Exit status 0 if crates.io has this crate at this version, 1 if not; any other answer stops.
on_crates_io() {
    local status
    status="$(curl -sS -o /dev/null -w '%{http_code}' --retry 3 \
        -A "witforge-release (https://github.com/asamarts/witforge)" \
        "https://crates.io/api/v1/crates/$1/$2")"
    case "${status}" in
        200) return 0 ;;
        404) return 1 ;;
        *) echo "crates.io answered HTTP ${status} for $1 $2" >&2; exit 1 ;;
    esac
}

missing=()
for crate in "${CRATES[@]}"; do
    version="$(version_of "${crate}")"
    if [[ -z "${version}" ]]; then
        echo "${crate} is not a workspace package" >&2
        exit 1
    fi
    if on_crates_io "${crate}" "${version}"; then
        echo "${crate} ${version} is already on crates.io" >&2
    else
        missing+=("${crate}")
    fi
done

if [[ "${mode}" == check ]]; then
    if [[ ${#missing[@]} -gt 0 ]]; then
        printf '%s\n' "${missing[@]}"
    fi
    exit 0
fi
if [[ ${#missing[@]} -eq 0 ]]; then
    echo "nothing to publish"
    exit 0
fi

packages=()
for crate in "${missing[@]}"; do
    packages+=(-p "${crate}")
done

# cargo verifies the CLI against the library through a temporary registry, and treats a
# registry crate as immutable: once it has unpacked the library into
# $CARGO_HOME/registry/src/-<hash>/ and built it, a later run of the same version reuses both,
# even after the source changed, and verifies against the old library. Remove those copies and
# verify in a target directory of our own, so a local run checks the source being published,
# as a fresh CI runner does.
cargo_home="${CARGO_HOME:-${HOME}/.cargo}"
for crate in "${CRATES[@]}"; do
    crate_version="$(version_of "${crate}")"
    rm -rf "${cargo_home}/registry/src/"-*/"${crate}-${crate_version}"
done
target="${ROOT}/target/publish"
rm -rf "${target}"

if [[ "${mode}" == dry-run ]]; then
    cargo publish --locked --target-dir "${target}" --dry-run "${packages[@]}"
else
    cargo publish --locked --target-dir "${target}" "${packages[@]}"
fi
