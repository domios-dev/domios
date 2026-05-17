#!/usr/bin/env bash
# Package a notarized CodeTerm.app into a .app.tar.gz + .sig pair for the
# tauri-plugin-updater channel manifest.
#
# Catches the bug that bit v1.3.0: without --no-mac-metadata, BSD tar embeds
# com.apple.* xattrs as PAX headers, which the Rust tar crate (used by
# tauri-updater) extracts as `._CodeTerm.app` AppleDouble files alongside the
# bundle — and the updater rejects with "failed to unpack '._CodeTerm.app'".
#
# Echoes the base64-encoded signature to stdout (no trailing newline) so the
# caller can substitute it into latest.json. The .sig file is also persisted
# next to the .tar.gz for archive but is NOT uploaded to the release (per
# skill rules — the signature already lives inline in latest.json).
#
# Usage: pack-mac-updater.sh <ver> <arch> <work-dir>
#   arch: aarch64 | x64

set -euo pipefail

VER="${1:-}"
ARCH="${2:-}"
WORK="${3:-}"
[ -n "$VER" ] && [ -n "$ARCH" ] && [ -n "$WORK" ] \
  || { echo "usage: pack-mac-updater.sh <ver> <arch> <work-dir>" >&2; exit 2; }

REPO_ROOT="$(cd "$(dirname "$0")/../../../.." && pwd)"
case "$ARCH" in
  aarch64) APP_DIR="$REPO_ROOT/src-tauri/target/release/bundle/macos" ;;
  x64)     APP_DIR="$REPO_ROOT/src-tauri/target/x86_64-apple-darwin/release/bundle/macos" ;;
  *) echo "unknown arch: $ARCH (expected aarch64 or x64)" >&2; exit 2 ;;
esac
[ -d "$APP_DIR/CodeTerm.app" ] || { echo "ERROR: $APP_DIR/CodeTerm.app not found" >&2; exit 1; }
mkdir -p "$WORK"
OUT="$WORK/CodeTerm_${VER}_${ARCH}.app.tar.gz"

# --no-mac-metadata is mandatory: see top-of-file comment.
tar --no-mac-metadata -czf "$OUT" -C "$APP_DIR" CodeTerm.app

# Sanity check: a non-BSD extractor must NOT materialize ._CodeTerm.app.
TESTDIR=$(mktemp -d)
python3 -c "import tarfile; tarfile.open('$OUT').extractall('$TESTDIR')" 2>/dev/null
if ls "$TESTDIR" | grep -q '^\._'; then
  echo "ERROR: AppleDouble leak in $OUT — extract produced ._* files" >&2
  rm -rf "$TESTDIR"
  exit 1
fi
rm -rf "$TESTDIR"

# Sign with minisign. The signature file content (base64) goes inline in
# latest.json. The .sig file itself is NOT uploaded to the release.
KEY_B64=$(cat "$HOME/.tauri/codeterm.key" | tr -d '\n')
TAURI_SIGNING_PRIVATE_KEY="$KEY_B64" TAURI_SIGNING_PRIVATE_KEY_PASSWORD="" \
  npx --yes @tauri-apps/cli signer sign "$OUT" >/dev/null

cat "$OUT.sig" | tr -d '\n'
