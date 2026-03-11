#!/usr/bin/env bash
# setup.sh — One-time setup for nix-apptainer
# Creates a sparse ext3 overlay image for persistent Nix state.
#
# Usage: ./setup.sh [--size SIZE_MB] [--overlay PATH] [--sif PATH]

set -euo pipefail

# --- Defaults ---
OVERLAY_SIZE=51200  # 50 GB (sparse — only uses actual written space)
OVERLAY_PATH="./nix-overlay.img"
SIF_PATH="./base-nixos.sif"

# --- Parse arguments ---
while [[ $# -gt 0 ]]; do
    case "$1" in
        --size)
            OVERLAY_SIZE="$2"
            shift 2
            ;;
        --overlay)
            OVERLAY_PATH="$2"
            shift 2
            ;;
        --sif)
            SIF_PATH="$2"
            shift 2
            ;;
        -h|--help)
            echo "Usage: $0 [--size SIZE_MB] [--overlay PATH] [--sif PATH]"
            echo ""
            echo "Options:"
            echo "  --size     Overlay size in MB (default: 51200 = 50GB, sparse)"
            echo "  --overlay  Path for the overlay image (default: ./nix-overlay.img)"
            echo "  --sif      Path to the base SIF image (default: ./base-nixos.sif)"
            exit 0
            ;;
        *)
            echo "Unknown option: $1"
            exit 1
            ;;
    esac
done

# --- Validate ---
if [ ! -f "$SIF_PATH" ]; then
    echo "Error: SIF image not found at $SIF_PATH"
    echo "Copy the base-nixos.sif image to this directory, or use --sif to specify its path."
    exit 1
fi

if [ -f "$OVERLAY_PATH" ]; then
    echo "Warning: Overlay already exists at $OVERLAY_PATH"
    read -rp "Overwrite? [y/N] " confirm
    if [[ "$confirm" != [yY] ]]; then
        echo "Aborted."
        exit 0
    fi
    rm -f "$OVERLAY_PATH"
fi

# --- Create sparse overlay ---
echo "Creating sparse ext3 overlay (${OVERLAY_SIZE} MB)..."
apptainer overlay create --sparse --size "$OVERLAY_SIZE" "$OVERLAY_PATH"
echo "Overlay created at $OVERLAY_PATH"

echo ""
echo "Setup complete! Enter the container with:"
echo "  ./enter.sh"
