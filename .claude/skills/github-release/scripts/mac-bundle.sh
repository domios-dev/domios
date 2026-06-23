#!/usr/bin/env bash
# Full macOS-per-arch pipeline: build → notarize+staple DMG → pack .app.tar.gz
# updater bundle → minisign-sign. Emits the final base64 signature to stdout
# so the caller can substitute it into latest.json directly.
#
# This replaces ~5 sequential helper invocations and the manual sig-extract
# step. Hard-coded conventions (paths, identity, work dir) are documented
# inline; override via env if needed.
#
# Usage: mac-bundle.sh <ver> <arch> <work-dir>
#   arch: arm64 | x86_64

set -euo pipefail

VER="${1:-}"
ARCH="${2:-}"
WORK="${3:-}"
[ -n "$VER" ] && [ -n "$ARCH" ] && [ -n "$WORK" ] \
  || { echo "usage: mac-bundle.sh <ver> <arch> <work-dir>" >&2; exit 2; }

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

case "$ARCH" in
  arm64|aarch64)
    BUILD_ARG=arm64
    DMG_ARCH=aarch64
    DMG=src-tauri/target/release/bundle/dmg/CodeTerm_${VER}_aarch64.dmg
    ;;
  x86_64|x64)
    BUILD_ARG=x86_64
    DMG_ARCH=x64
    DMG=src-tauri/target/x86_64-apple-darwin/release/bundle/dmg/CodeTerm_${VER}_x64.dmg
    ;;
  *) echo "unknown arch: $ARCH (expected arm64 or x86_64)" >&2; exit 2 ;;
esac

# All interim output to stderr so the final stdout line is JUST the sig.
echo "=== build $ARCH ===" >&2
"$SCRIPT_DIR/build-mac.sh" "$BUILD_ARG" >&2

echo "=== notarize+staple $DMG ===" >&2
"$SCRIPT_DIR/notarize-dmg.sh" "$DMG" >&2

echo "=== pack updater bundle $DMG_ARCH ===" >&2
SIG=$("$SCRIPT_DIR/pack-mac-updater.sh" "$VER" "$DMG_ARCH" "$WORK")

echo "=== done: $DMG ($DMG_ARCH) ===" >&2
printf '%s' "$SIG"
