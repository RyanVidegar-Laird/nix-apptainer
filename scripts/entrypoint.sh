#!/usr/bin/env bash
# entrypoint.sh — Apptainer container entrypoint
# Handles first-run Nix store DB initialization and shell setup.

set -euo pipefail

# Ensure PATH includes nix and system tools (needed for the TERM check;
# the login shell will get full PATH from /etc/profile)
export PATH="/usr/local/bin:/run/sw/bin:/bin:/usr/bin:${PATH:-}"
export NIX_REMOTE=""

# Fall back to xterm-256color if TERM is unset or unrecognized
if [ -z "${TERM:-}" ] || ! infocmp "$TERM" >/dev/null 2>&1; then
    export TERM=xterm-256color
fi

# --- Execute command or interactive shell ---
if [ $# -gt 0 ]; then
    exec "$@"
else
    exec /bin/bash --login
fi
