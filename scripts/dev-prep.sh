#!/usr/bin/env bash
# Pre-dev preparation. Replaces the inline beforeDevCommand chain that
# was rebuilding codeterm-embed on every cycle (70+ seconds) even when
# nothing in src-tauri changed. With this script:
#
#   - CLI still rebuilds (fast incremental, 5-6s)
#   - codeterm-embed skipped if the bundled binary is already present
#     and >1MB (i.e. a real binary, not the build-time stub).
#     Force-rebuild via:  REBUILD_EMBED=1 ./build.sh dev
#     OR explicitly:      ./build.sh embed
#
# Cuts dev cycle from ~98s to ~25-30s for src-tauri-only edits.

set -e

REPO="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO"

triple=$(rustc -vV | awk -F': ' '/^host:/{print $2}')
EXE_EXT=""
case "$OSTYPE" in msys*|cygwin*|win32) EXE_EXT=".exe" ;; esac
EMBED_BUNDLE="src-tauri/bundled-embed/codeterm-embed-${triple}${EXE_EXT}"

# --- 1. CLI ---
touch cli/src/main.rs
cargo build --release --manifest-path cli/Cargo.toml
node scripts/bundle-cli.js

# --- 2. embed stub (always — guarantees the file exists for tauri_build) ---
node scripts/prepare-embed-stub.js

# --- 3. codeterm-embed binary — conditional ---
embed_size=0
[ -f "$EMBED_BUNDLE" ] && embed_size=$(stat -c %s "$EMBED_BUNDLE" 2>/dev/null || stat -f %z "$EMBED_BUNDLE" 2>/dev/null || echo 0)

if [ "${REBUILD_EMBED:-0}" = "1" ] || [ "$embed_size" -lt 1048576 ]; then
  echo "[dev-prep] building codeterm-embed (size=${embed_size}, REBUILD_EMBED=${REBUILD_EMBED:-0})"
  cargo build --release --manifest-path src-tauri/Cargo.toml --bin codeterm-embed
  node scripts/bundle-embed.js
else
  echo "[dev-prep] skipping codeterm-embed rebuild (bundle exists, ${embed_size} bytes; set REBUILD_EMBED=1 to force)"
fi

# --- 4. hand off to vite ---
exec pnpm dev
