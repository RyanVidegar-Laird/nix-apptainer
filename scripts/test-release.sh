#!/usr/bin/env bash
# test-release.sh — Test the release workflow locally with act
#
# Usage:
#   scripts/test-release.sh           # Run build job only (x86_64-linux)
#   scripts/test-release.sh --full    # Run build + release jobs
#
# Requires .secrets/env with GPG_KEY_ID, GPG_PASSPHRASE, GPG_PRIVATE_KEY

set -euo pipefail

# Podman support: point act at the rootless socket
if [ -S "${XDG_RUNTIME_DIR:-/run/user/$(id -u)}/podman/podman.sock" ]; then
    PODMAN_SOCK="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}/podman/podman.sock"
    export DOCKER_HOST="unix://$PODMAN_SOCK"
fi

SECRETS_FILE="${SECRETS_FILE:-.secrets/env}"

if [ ! -f "$SECRETS_FILE" ]; then
    echo "Error: $SECRETS_FILE not found" >&2
    echo "Create it with GPG_KEY_ID, GPG_PASSPHRASE, GPG_PRIVATE_KEY exports" >&2
    exit 1
fi

# shellcheck source=/dev/null
source "$SECRETS_FILE"

ACT_ARGS=(
    push
    -s "GPG_KEY_ID" -s "GPG_PASSPHRASE" -s "GPG_PRIVATE_KEY"
    --eventpath /dev/stdin
)

if [ -n "${PODMAN_SOCK:-}" ]; then
    ACT_ARGS+=(--container-daemon-socket "$PODMAN_SOCK")
fi

if [ "${1:-}" = "--full" ]; then
    echo "Running full release workflow (build + release)..."
else
    echo "Running build job only (x86_64-linux)..."
    ACT_ARGS+=(-j build --matrix arch:x86_64-linux)
fi

echo '{"ref": "refs/tags/v0.1.0"}' | act "${ACT_ARGS[@]}"
