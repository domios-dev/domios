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

[ -f "$KEY" ] || { echo "minisign key missing at $KEY (restore from Bitwarden slug codeterm-tauri-minisign-private-key)" >&2; exit 1; }

# Export TAURI_SIGNING_PRIVATE_KEY BEFORE the build so the Tauri bundler emits
# the .nsis.zip itself (with ZIP_STORED, the only compression tauri-updater
# accepts on Windows). Without this env, the bundler skips the updater bundle
# and we fall back to repacking, which is fragile.
KEY_B64="$(tr -d '\n' < "$KEY")"
export TAURI_SIGNING_PRIVATE_KEY="$KEY_B64"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD=""

echo "=== build $VER (windows x64) ===" >&2
./build.sh build >&2

if [ ! -f "$EXE" ]; then
  echo "expected NSIS setup not found: $EXE" >&2
  exit 1
fi

if [ ! -f "$ZIP" ]; then
  echo "=== fallback: python zipfile ZIP_STORED (bundler did not emit .nsis.zip) ===" >&2
  # DO NOT use powershell Compress-Archive — its -CompressionLevel always emits
  # DEFLATE, which tauri-updater rejects with "Compression method not supported".
  # python zipfile.ZIP_STORED writes method=0 which is what the updater needs.
  python -c "
import zipfile, sys, os
src, dst = sys.argv[1], sys.argv[2]
with zipfile.ZipFile(dst, 'w', zipfile.ZIP_STORED) as z:
    z.write(src, arcname=os.path.basename(src))
" "$EXE" "$ZIP" >&2 || python3 -c "
import zipfile, sys, os
src, dst = sys.argv[1], sys.argv[2]
with zipfile.ZipFile(dst, 'w', zipfile.ZIP_STORED) as z:
    z.write(src, arcname=os.path.basename(src))
" "$EXE" "$ZIP" >&2
fi

[ -f "$ZIP" ] || { echo "failed to produce $ZIP" >&2; exit 1; }

# Hard guard against the v1.1.22 / v1.3.2 trap: verify bytes 8-9 of the local
# file header == 00 00 (STORED). Anything else (08 00 = DEFLATE, 09 00 =
# Deflate64) makes tauri-updater bail with "Compression method not supported".
METHOD_HEX="$(od -A n -t x1 -N 10 "$ZIP" | tr -d ' \n' | cut -c17-20)"
if [ "$METHOD_HEX" != "0000" ]; then
  echo "BLOCKED: $ZIP compression method=0x$METHOD_HEX (expected 0000 STORED)" >&2
  echo "  tauri-updater accepts STORED only. Fix: re-pack with python zipfile.ZIP_STORED." >&2
  exit 1
fi
echo "=== verified $ZIP compression=STORED ===" >&2

echo "=== sign $ZIP ===" >&2
node_modules/.bin/tauri signer sign "$ZIP" >&2

[ -f "$SIG" ] || { echo "signer did not produce $SIG" >&2; exit 1; }

echo "=== done: $ZIP ===" >&2
tr -d '\n' < "$SIG"
