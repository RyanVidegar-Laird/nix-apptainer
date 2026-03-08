#!/usr/bin/env bash
# entrypoint.sh — Apptainer container entrypoint
# Handles first-run Nix store DB initialization and shell setup.

set -euo pipefail

# --- First-run: initialize Nix store database ---
# The nix-path-registration file contains the base image's store path
# metadata. We load it into the SQLite database on first run so that
# nix knows about all the pre-installed packages.
if [ -f /nix-path-registration ] && [ ! -f /nix/var/nix/db/db.sqlite ]; then
    echo "nix-apptainer: First run detected. Initializing Nix store database..."
    /usr/local/bin/nix-store --load-db < /nix-path-registration
    echo "nix-apptainer: Store database initialized."
fi

# --- Source environment ---
if [ -f /etc/profile ]; then
    # shellcheck disable=SC1091
    . /etc/profile
fi

# Ensure PATH includes nix and system tools
export PATH="/usr/local/bin:/run/sw/bin:/bin:/usr/bin:${PATH:-}"
export NIX_REMOTE=""

# --- Execute command or interactive shell ---
if [ $# -gt 0 ]; then
    exec "$@"
else
    exec /bin/bash --login
fi
