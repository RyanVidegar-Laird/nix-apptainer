#!/usr/bin/env bash
# sign-release.sh — Sign CLI binary and SIF image for release
#
# Usage:
#   scripts/sign-release.sh [OPTIONS]
#
# Options:
#   --arch ARCH        Target arch (default: detect from uname -m)
#   --cli PATH         Path to CLI binary (default: cli-result/bin/nix-apptainer)
#   --sif PATH         Path to SIF image (default: sif-result)
#   --output-dir DIR   Output directory (default: release/)
#   -h, --help         Show this help
#
# Environment variables (all optional):
#   GPG_KEY_ID         GPG key fingerprint/ID for signing
#   GPG_PASSPHRASE     Passphrase for non-interactive signing (CI)
#   GPG_PRIVATE_KEY    Armored private key to import (CI)
#
# In CI mode (GPG_PRIVATE_KEY set), the key is imported into both GPG and
# apptainer keyrings automatically. In local mode, keys are assumed to
# already exist in your keyrings.

set -euo pipefail

# --- Defaults ---
ARCH=""
CLI_PATH="cli-result/bin/nix-apptainer"
SIF_PATH="sif-result"
OUTPUT_DIR="release"

# --- Argument parsing ---
while [[ $# -gt 0 ]]; do
    case "$1" in
        --arch)
            ARCH="$2"
            shift 2
            ;;
        --cli)
            CLI_PATH="$2"
            shift 2
            ;;
        --sif)
            SIF_PATH="$2"
            shift 2
            ;;
        --output-dir)
            OUTPUT_DIR="$2"
            shift 2
            ;;
        -h|--help)
            sed -n '2,/^$/{ s/^# \?//; p }' "$0"
            exit 0
            ;;
        *)
            echo "Unknown option: $1" >&2
            exit 1
            ;;
    esac
done

# --- Detect arch ---
if [ -z "$ARCH" ]; then
    machine="$(uname -m)"
    case "$machine" in
        x86_64)  ARCH="x86_64-linux" ;;
        aarch64) ARCH="aarch64-linux" ;;
        *)       ARCH="${machine}-linux" ;;
    esac
fi

# --- Validate inputs ---
if [ ! -f "$CLI_PATH" ]; then
    echo "Error: CLI binary not found at $CLI_PATH" >&2
    echo "Build with: nix build .#cli -o cli-result" >&2
    exit 1
fi

if [ ! -e "$SIF_PATH" ]; then
    echo "Error: SIF image not found at $SIF_PATH" >&2
    echo "Build with: nix build -o sif-result" >&2
    exit 1
fi

# Resolve SIF path if it's a symlink (nix build output)
SIF_PATH="$(readlink -f "$SIF_PATH")"

# --- GPG passphrase args ---
GPG_BATCH_ARGS=()
if [ -n "${GPG_PASSPHRASE:-}" ]; then
    GPG_BATCH_ARGS=(--batch --yes --passphrase-fd 0 --pinentry-mode loopback)
fi

if [ -n "${GPG_KEY_ID:-}" ]; then
    GPG_BATCH_ARGS+=(--default-key "$GPG_KEY_ID")
fi

gpg_sign() {
    local input="$1" output="$2"
    if [ -n "${GPG_PASSPHRASE:-}" ]; then
        echo "$GPG_PASSPHRASE" | \
            gpg "${GPG_BATCH_ARGS[@]}" --detach-sign --armor -o "$output" "$input"
    elif [ ${#GPG_BATCH_ARGS[@]} -gt 0 ]; then
        gpg "${GPG_BATCH_ARGS[@]}" --detach-sign --armor -o "$output" "$input"
    else
        gpg --detach-sign --armor -o "$output" "$input"
    fi
}

# --- Setup output directory ---
mkdir -p "$OUTPUT_DIR"

# --- Import keys (CI mode: GPG_PRIVATE_KEY set) ---
if [ -n "${GPG_PRIVATE_KEY:-}" ]; then
    echo "==> Importing GPG private key into gpg keyring"
    echo "$GPG_PRIVATE_KEY" | gpg --batch --import
    gpgconf --kill gpg-agent

    echo "==> Importing private key into apptainer keyring"
    tmpkey="$(mktemp)"
    echo "$GPG_PRIVATE_KEY" > "$tmpkey"
    if [ -n "${GPG_PASSPHRASE:-}" ]; then
        if ! output=$(echo "$GPG_PASSPHRASE" | apptainer key import "$tmpkey" 2>&1); then
            if echo "$output" | grep -q "already belongs to the keyring"; then
                echo "    (already imported, skipping)"
            else
                echo "$output" >&2
                rm -f "$tmpkey"
                exit 1
            fi
        fi
    else
        if ! output=$(apptainer key import "$tmpkey" 2>&1); then
            if echo "$output" | grep -q "already belongs to the keyring"; then
                echo "    (already imported, skipping)"
            else
                echo "$output" >&2
                rm -f "$tmpkey"
                exit 1
            fi
        fi
    fi
    rm -f "$tmpkey"

    echo "==> Importing public key into apptainer keyring"
    tmpkey="$(mktemp)"
    gpg --batch --armor --export "${GPG_KEY_ID:-}" > "$tmpkey"
    if ! output=$(apptainer key import "$tmpkey" 2>&1); then
        if echo "$output" | grep -q "already belongs to the keyring"; then
            echo "    (already imported, skipping)"
        else
            echo "$output" >&2
            rm -f "$tmpkey"
            exit 1
        fi
    fi
    rm -f "$tmpkey"
else
    echo "==> GPG_PRIVATE_KEY not set, assuming keys are already in keyrings"
fi

# --- Step 1: Copy and sign CLI binary ---
CLI_NAME="nix-apptainer-${ARCH}"
echo "Copying CLI binary → ${CLI_NAME}"
cp "$CLI_PATH" "${OUTPUT_DIR}/${CLI_NAME}"

echo "GPG signing CLI binary → ${CLI_NAME}.sig"
gpg_sign "${OUTPUT_DIR}/${CLI_NAME}" "${OUTPUT_DIR}/${CLI_NAME}.sig"

# --- Step 2: Copy and sign SIF ---
SIF_NAME="base-nixos-${ARCH}.sif"
echo "Copying SIF image → ${SIF_NAME}"
cp "$SIF_PATH" "${OUTPUT_DIR}/${SIF_NAME}"
chmod u+w "${OUTPUT_DIR}/${SIF_NAME}"

echo "Apptainer signing SIF → ${SIF_NAME}"
if [ -n "${GPG_PASSPHRASE:-}" ]; then
    echo "$GPG_PASSPHRASE" | apptainer sign --keyidx 0 "${OUTPUT_DIR}/${SIF_NAME}"
else
    apptainer sign "${OUTPUT_DIR}/${SIF_NAME}"
fi

# --- Step 3: Generate checksums ---
echo "Generating SHA256SUMS..."
(cd "$OUTPUT_DIR" && sha256sum "$CLI_NAME" "$CLI_NAME.sig" "$SIF_NAME" > SHA256SUMS)

echo "GPG signing checksums → SHA256SUMS.sig"
gpg_sign "${OUTPUT_DIR}/SHA256SUMS" "${OUTPUT_DIR}/SHA256SUMS.sig"

# --- Step 4: Copy signing key ---
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SIGNING_KEY="${SCRIPT_DIR}/../signing-key.asc"
if [ -f "$SIGNING_KEY" ]; then
    src="$(readlink -f "$SIGNING_KEY")"
    dst="$(readlink -f "${OUTPUT_DIR}/signing-key.asc" 2>/dev/null || echo "")"
    if [ "$src" = "$dst" ]; then
        echo "signing-key.asc already in output directory"
    else
        echo "Copying signing-key.asc"
        cp "$SIGNING_KEY" "${OUTPUT_DIR}/signing-key.asc"
    fi
else
    echo "Warning: signing-key.asc not found at ${SIGNING_KEY}, skipping" >&2
fi

# --- Summary ---
echo ""
echo "=== Release artifacts (${ARCH}) ==="
ls -lh "${OUTPUT_DIR}/"
echo ""
echo "Verify with:"
echo "  gpg --verify ${OUTPUT_DIR}/${CLI_NAME}.sig ${OUTPUT_DIR}/${CLI_NAME}"
echo "  apptainer verify ${OUTPUT_DIR}/${SIF_NAME}"
echo "  gpg --verify ${OUTPUT_DIR}/SHA256SUMS.sig ${OUTPUT_DIR}/SHA256SUMS"
echo "  (cd ${OUTPUT_DIR} && sha256sum -c SHA256SUMS)"
