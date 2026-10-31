#!/usr/bin/env bash
# Stop and remove witforge's runner service (see ci/README.md).
#
#   ci/runner/teardown.sh           stop the service and remove its unit; keep the registration
#   ci/runner/teardown.sh --purge   also deregister the runner and delete its volumes and image
set -euo pipefail

REPO="${WITFORGE_REPO:-asamarts/witforge}"
GH_USER="${WITFORGE_GH_USER:-asamarts}"
NAME=witforge-runner
IMAGE=localhost/witforge-runner:latest
UNIT_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/containers/systemd"
export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}"

purge=false
case "${1:-}" in
    "") ;;
    --purge) purge=true ;;
    *) echo "usage: $0 [--purge]" >&2; exit 2 ;;
esac

if [[ "${purge}" == true ]]; then
    # Find the registration before stopping anything: deleting the config volume without
    # deregistering would leave a runner on GitHub that nothing can remove with a token.
    GH_TOKEN="$(gh auth token --user "${GH_USER}")"
    export GH_TOKEN
    id="$(gh api "repos/${REPO}/actions/runners" \
        --jq ".runners[] | select(.name == \"${NAME}\") | .id")"
    if [[ -z "${id}" ]]; then
        echo "no runner named ${NAME} is registered on ${REPO}; set WITFORGE_REPO to the" \
            "repository it was registered on (nothing was changed)" >&2
        exit 1
    fi
fi

echo "==> Stopping ${NAME}.service"
systemctl --user stop "${NAME}.service" 2>/dev/null || true
rm -f "${UNIT_DIR}/${NAME}.container"
systemctl --user daemon-reload

if [[ "${purge}" == true ]]; then
    # Through the API: `config.sh remove` needs a registration token that has long expired.
    gh api -X DELETE "repos/${REPO}/actions/runners/${id}"
    echo "==> Deregistered ${NAME} from ${REPO}"
    podman volume rm -f "${NAME}-config" "${NAME}-cargo" >/dev/null
    podman image rm -f "${IMAGE}" >/dev/null 2>&1 || true
    echo "==> Removed the volumes and the image"
fi
echo "==> Teardown complete"
