#!/usr/bin/env bash
# enter.sh — Enter the nix-apptainer container
#
# Usage:
#   ./enter.sh              # Interactive shell
#   ./enter.sh --nv         # With NVIDIA GPU passthrough
#   ./enter.sh exec CMD     # Run a command

set -euo pipefail

# --- Configuration ---
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
OVERLAY_PATH="${NIX_APPTAINER_OVERLAY:-$SCRIPT_DIR/nix-overlay.img}"
SIF_PATH="${NIX_APPTAINER_SIF:-$SCRIPT_DIR/base-nixos.sif}"

# --- Validate ---
if [ ! -f "$SIF_PATH" ]; then
    echo "Error: SIF image not found at $SIF_PATH"
    echo "Set NIX_APPTAINER_SIF or copy base-nixos.sif to $SCRIPT_DIR"
    exit 1
fi

if [ ! -f "$OVERLAY_PATH" ]; then
    echo "Error: Overlay not found at $OVERLAY_PATH"
    echo "Run ./setup.sh first, or set NIX_APPTAINER_OVERLAY"
    exit 1
fi

# --- Build apptainer arguments ---
APPTAINER_ARGS=(
    --overlay "$OVERLAY_PATH"
)

# Collect extra flags (like --nv, --bind, etc.)
EXTRA_ARGS=()
EXEC_MODE="shell"
EXEC_CMD=()

while [[ $# -gt 0 ]]; do
    case "$1" in
        --nv|--rocm)
            APPTAINER_ARGS+=("$1")
            shift
            ;;
        --bind|-B)
            APPTAINER_ARGS+=("$1" "$2")
            shift 2
            ;;
        exec)
            EXEC_MODE="exec"
            shift
            EXEC_CMD=("$@")
            break
            ;;
        -h|--help)
            echo "Usage: $0 [OPTIONS] [exec COMMAND...]"
            echo ""
            echo "Options:"
            echo "  --nv       Enable NVIDIA GPU passthrough"
            echo "  --rocm     Enable AMD ROCm GPU passthrough"
            echo "  --bind SRC:DST  Bind-mount a host path into the container"
            echo "  exec CMD   Run CMD instead of an interactive shell"
            echo ""
            echo "Environment variables:"
            echo "  NIX_APPTAINER_SIF      Path to .sif image"
            echo "  NIX_APPTAINER_OVERLAY  Path to overlay image"
            exit 0
            ;;
        *)
            EXTRA_ARGS+=("$1")
            shift
            ;;
    esac
done

# --- Enter container ---
if [ "$EXEC_MODE" = "exec" ]; then
    exec apptainer exec \
        "${APPTAINER_ARGS[@]}" \
        "${EXTRA_ARGS[@]}" \
        "$SIF_PATH" \
        "${EXEC_CMD[@]}"
else
    exec apptainer shell \
        "${APPTAINER_ARGS[@]}" \
        "${EXTRA_ARGS[@]}" \
        "$SIF_PATH"
fi
