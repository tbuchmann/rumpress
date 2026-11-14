#!/usr/bin/env bash
# Build a downloadable release tarball:
#
#   dist/rumpress-<version>-x86_64-linux.tar.gz
#   dist/rumpress-<version>-x86_64-linux.tar.gz.sha256
#
# The tarball unpacks to one folder holding the stripped release binary, the
# Pdfium library it needs (rumpress looks for libpdfium.so next to its own
# executable), and the licenses.
#
# Usage: scripts/package-release.sh [VERSION]
#   VERSION defaults to the version in Cargo.toml.
set -euo pipefail

if [[ "$(uname -s)-$(uname -m)" != "Linux-x86_64" ]]; then
    echo "error: release packaging is only set up for Linux x86_64" >&2
    echo "       (scripts/fetch-pdfium.sh downloads the x64 Pdfium build)" >&2
    exit 1
fi

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

VERSION="${1:-$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)}"
if [[ -z "$VERSION" ]]; then
    echo "error: could not determine the version" >&2
    exit 1
fi

NAME="rumpress-${VERSION}-x86_64-linux"
DIST="$ROOT/dist"
STAGE="$DIST/$NAME"

echo "==> Fetching Pdfium"
"$ROOT/scripts/fetch-pdfium.sh"

echo "==> Building release binary"
cargo build --release --locked

echo "==> Assembling $NAME"
rm -rf "$STAGE" "$DIST/$NAME.tar.gz" "$DIST/$NAME.tar.gz.sha256"
mkdir -p "$STAGE/third-party/pdfium"
install -m 755 target/release/rumpress "$STAGE/rumpress"
install -m 755 vendor/pdfium/libpdfium.so "$STAGE/libpdfium.so"
install -m 644 README.md LICENSE "$STAGE/"
install -m 644 vendor/pdfium/LICENSE vendor/pdfium/VERSION "$STAGE/third-party/pdfium/"
cp -r vendor/pdfium/licenses "$STAGE/third-party/pdfium/licenses"

echo "==> Creating tarball"
tar -C "$DIST" --owner=0 --group=0 -czf "$DIST/$NAME.tar.gz" "$NAME"
(cd "$DIST" && sha256sum "$NAME.tar.gz" > "$NAME.tar.gz.sha256")

echo
echo "Done:"
ls -lh "$DIST/$NAME.tar.gz" "$DIST/$NAME.tar.gz.sha256"
cat "$DIST/$NAME.tar.gz.sha256"
