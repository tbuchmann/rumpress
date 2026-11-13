#!/usr/bin/env bash
# Download the Linux x64 Pdfium build from bblanchon/pdfium-binaries GitHub
# releases into vendor/pdfium/ so rumpress can bind to libpdfium.so.
set -euo pipefail

VENDOR_DIR="$(cd "$(dirname "$0")/.." && pwd)/vendor/pdfium"
mkdir -p "$VENDOR_DIR"

if [[ -f "$VENDOR_DIR/libpdfium.so" ]]; then
    echo "libpdfium.so already present in $VENDOR_DIR"
    exit 0
fi

URL="https://github.com/bblanchon/pdfium-binaries/releases/latest/download/pdfium-linux-x64.tgz"
TGZ="$(mktemp -t pdfium-XXXXXX.tgz)"
trap 'rm -f "$TGZ"' EXIT

echo "Downloading $URL ..."
curl -fsSL "$URL" -o "$TGZ"

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP" "$TGZ"' EXIT
tar -xzf "$TGZ" -C "$TMP"

cp "$TMP"/lib/libpdfium.so "$VENDOR_DIR/libpdfium.so"
echo "Installed $VENDOR_DIR/libpdfium.so"
