#!/usr/bin/env bash
# Build and package DownloadCraft for FreeBSD:
#
#   $DIST/downloadcraft-<version>-freebsd-<arch>.tar.gz   bin/, share/ (desktop entry, icons, AppStream)
#
# Usage: packaging/freebsd/package.sh [--skip-build]
#
# Run on FreeBSD (CI: a FreeBSD 15 VM via vmactions/freebsd-vm). Runtime needs: libxkbcommon,
# wayland or libX11 and mesa (Vulkan or EGL): `pkg install libxkbcommon libX11 mesa-libs vulkan-loader`.
set -euo pipefail
# shellcheck source=../common.sh
. "$(dirname "${BASH_SOURCE[0]}")/../common.sh"
LINUX="$ROOT/packaging/linux"
APP_ID=ai.storyteller.downloadcraft

if [ "${1:-}" != "--skip-build" ]; then
  (cd "$ROOT" && cargo build --release --locked -p downloadcraft -p downloadcraft-cli)
fi

ARCH="$(uname -m)"
case "$ARCH" in
  amd64) ARCH=x86_64 ;;
  arm64) ARCH=aarch64 ;;
esac
BIN="$CARGO_TARGET_DIR/release"
NAME="downloadcraft-$VERSION-freebsd-$ARCH"
WORK="$CARGO_TARGET_DIR/freebsd-package"
STAGE="$WORK/$NAME"
rm -rf "$WORK"
mkdir -p "$STAGE/bin" "$STAGE/share/applications" "$STAGE/share/metainfo" "$STAGE/share/icons" "$STAGE/share/doc/downloadcraft"

install -m 755 "$BIN/downloadcraft" "$BIN/downloadcraft-cli" "$STAGE/bin/"
strip "$STAGE/bin/downloadcraft" "$STAGE/bin/downloadcraft-cli" 2>/dev/null || true
install -m 644 "$LINUX/$APP_ID.desktop" "$STAGE/share/applications/"
sed -e "s/@VERSION@/$VERSION/g" -e "s/@DATE@/$DOWNLOADCRAFT_BUILD_DATE/g" \
  "$LINUX/$APP_ID.metainfo.xml.in" >"$STAGE/share/metainfo/$APP_ID.metainfo.xml"
cp -R "$ROOT/assets/app-icon/hicolor" "$STAGE/share/icons/"
copy_docs "$STAGE/share/doc/downloadcraft"

tar -C "$WORK" -czf "$DIST/$NAME.tar.gz" "$NAME"
"$STAGE/bin/downloadcraft-cli" --version
echo "wrote $DIST/$NAME.tar.gz"
ls -lh "$DIST/$NAME.tar.gz"
