#!/usr/bin/env bash
# Entrypoint of the witforge runner container (see ci/README.md).
#
#   entrypoint.sh register   register once with RUNNER_TOKEN and store the credentials
#   entrypoint.sh            run the agent with the stored credentials
#
# Registration is a separate one-off container so that the long-running service never holds a
# token: it only reads the credentials from the config volume.
set -euo pipefail

: "${GITHUB_REPO_URL:?GITHUB_REPO_URL is required}"
RUNNER_NAME="${RUNNER_NAME:-witforge-runner}"
RUNNER_LABELS="${RUNNER_LABELS:-witforge}"
CONFIG_DIR=/home/runner/_config
CREDENTIAL_FILES=(.runner .credentials .credentials_rsaparams)

cd /home/runner/actions-runner

if [[ "${1:-}" == "register" ]]; then
    : "${RUNNER_TOKEN:?RUNNER_TOKEN (a registration token) is required to register}"
    # Self-update stays enabled: GitHub stops queueing jobs for a runner left more than 30 days
    # behind a release, and an updated agent is simply re-fetched after a container recreate.
    ./config.sh \
        --url "${GITHUB_REPO_URL}" \
        --token "${RUNNER_TOKEN}" \
        --name "${RUNNER_NAME}" \
        --labels "${RUNNER_LABELS}" \
        --unattended \
        --replace
    for f in "${CREDENTIAL_FILES[@]}"; do
        cp "$f" "${CONFIG_DIR}/"
    done
    echo "==> Registered '${RUNNER_NAME}'; credentials stored in the config volume"
    exit 0
fi

if [[ ! -f "${CONFIG_DIR}/.runner" ]]; then
    echo "==> No stored registration; run ci/runner/setup.sh on the host" >&2
    exit 1
fi
for f in "${CREDENTIAL_FILES[@]}"; do
    cp "${CONFIG_DIR}/$f" .
done

# Stop the agent on a signal, without deregistering: SIGTERM arrives on every ordinary restart,
# and only `ci/runner/teardown.sh --purge` means the runner is going away.
# shellcheck disable=SC2329  # invoked through the trap
cleanup() {
    kill -TERM "${AGENT_PID:-0}" 2>/dev/null || true
}
trap cleanup SIGTERM SIGINT

AGENT_LOG="$(mktemp)"
set +e
# Process substitution, not a pipe: after a pipeline `$!` is the last element (tee), not the agent.
./run.sh > >(tee "$AGENT_LOG") 2>&1 &
AGENT_PID=$!
# `wait` returns as soon as a trap fires, before the agent has exited; wait until it has.
AGENT_STATUS=0
while :; do
    wait "$AGENT_PID"
    AGENT_STATUS=$?
    kill -0 "$AGENT_PID" 2>/dev/null || break
done
set -e

# A registration deleted on the server can never connect again; drop it so the next start
# fails with the setup hint above instead of looping on dead credentials.
if grep -qF 'registration has been deleted from the server' "$AGENT_LOG"; then
    echo "==> The registration was deleted on the server; discarding it" >&2
    for f in "${CREDENTIAL_FILES[@]}"; do
        rm -f "${CONFIG_DIR}/$f"
    done
fi
rm -f "$AGENT_LOG"
exit "$AGENT_STATUS"
