#!/usr/bin/env bash
# Upload the Windows release artifacts to both repos. Idempotent: if a release
# draft does not exist on the mirror, create it. Safe to re-run after partial
# failure — `gh release upload --clobber` replaces existing assets.
#
# Assumes win-bundle.sh has already produced the .exe + .nsis.zip in
# src-tauri/target/release/bundle/nsis/.
#
# Usage: win-upload.sh <ver>
#   stderr: gh logs
#   exit:   0 ok, non-zero blocker

set -euo pipefail

VER="${1:-}"
[ -n "$VER" ] || { echo "usage: win-upload.sh <ver>" >&2; exit 2; }

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
cd "$REPO_ROOT"

NSIS_DIR="src-tauri/target/release/bundle/nsis"
EXE="$NSIS_DIR/CodeTerm_${VER}_x64-setup.exe"
ZIP="$NSIS_DIR/CodeTerm_${VER}_x64-setup.nsis.zip"

[ -f "$EXE" ] || { echo "missing $EXE (run win-bundle.sh first)" >&2; exit 1; }
[ -f "$ZIP" ] || { echo "missing $ZIP (run win-bundle.sh first)" >&2; exit 1; }

upload_to() {
  local repo="$1"
  echo "=== upload to $repo ===" >&2
  if ! gh release view "v$VER" --repo "$repo" >/dev/null 2>&1; then
    echo "no release v$VER on $repo — creating draft" >&2
    gh release create "v$VER" --repo "$repo" --draft \
      --title "v$VER" --notes "v$VER" >&2
  fi
  gh release upload "v$VER" "$EXE" "$ZIP" --clobber --repo "$repo" >&2
}

upload_to rollacode/codeterm
upload_to rollacode/codeterm-releases

echo "=== uploaded $VER to private + mirror ===" >&2
