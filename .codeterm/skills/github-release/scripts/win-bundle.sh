#!/usr/bin/env bash
# Full Windows pipeline: ensure release build exists → ensure .nsis.zip is
# emitted (fallback to PowerShell Compress-Archive if the Tauri bundler
# skipped it) → minisign-sign the .nsis.zip with the key at ~/.tauri/codeterm.key.
# Emits the final base64 signature to stdout so the caller (win-release.sh)
# can drop it straight into latest.json.
#
# Assumes the tree is already at the v$VER tag (win-release.sh does the
# checkout). Mirrors mac-bundle.sh contract.
#
# Usage: win-bundle.sh <ver>
#   stdout: clean base64 sig (single line, no trailing newline)
#   stderr: build + zip + sign logs
#   exit:   0 ok, non-zero blocker

set -euo pipefail

VER="${1:-}"
[ -n "$VER" ] || { echo "usage: win-bundle.sh <ver>" >&2; exit 2; }

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
cd "$REPO_ROOT"

NSIS_DIR="src-tauri/target/release/bundle/nsis"
EXE="$NSIS_DIR/CodeTerm_${VER}_x64-setup.exe"
ZIP="$NSIS_DIR/CodeTerm_${VER}_x64-setup.nsis.zip"
SIG="$NSIS_DIR/CodeTerm_${VER}_x64-setup.nsis.zip.sig"
KEY="$HOME/.tauri/codeterm.key"

echo "=== build $VER (windows x64) ===" >&2
./build.sh build >&2

if [ ! -f "$EXE" ]; then
  echo "expected NSIS setup not found: $EXE" >&2
  exit 1
fi

if [ ! -f "$ZIP" ]; then
  echo "=== fallback: Compress-Archive (bundler did not emit .nsis.zip) ===" >&2
  powershell.exe -NoProfile -Command \
    "Compress-Archive -Path '$EXE' -DestinationPath '$ZIP' -CompressionLevel Optimal -Force" >&2
fi

[ -f "$ZIP" ] || { echo "failed to produce $ZIP" >&2; exit 1; }
[ -f "$KEY" ] || { echo "minisign key missing at $KEY (restore from Bitwarden slug codeterm-tauri-minisign-private-key)" >&2; exit 1; }

echo "=== sign $ZIP ===" >&2
KEY_B64="$(tr -d '\n' < "$KEY")"
TAURI_SIGNING_PRIVATE_KEY="$KEY_B64" \
TAURI_SIGNING_PRIVATE_KEY_PASSWORD="" \
  node_modules/.bin/tauri signer sign "$ZIP" >&2

[ -f "$SIG" ] || { echo "signer did not produce $SIG" >&2; exit 1; }

echo "=== done: $ZIP ===" >&2
tr -d '\n' < "$SIG"
