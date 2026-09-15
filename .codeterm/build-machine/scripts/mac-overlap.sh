#!/usr/bin/env bash
# Build the first macOS arch, notarize it in the background, then build the second arch.

set -euo pipefail

VER="${1:-}"
WORK="${2:-}"
[ -n "$VER" ] && [ -n "$WORK" ] || { echo "usage: mac-overlap.sh <ver> <work-dir>" >&2; exit 2; }

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"
HELPERS="$REPO_ROOT/.agents/skills/github-release/scripts"
cd "$REPO_ROOT"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$REPO_ROOT/src-tauri/target}"
export CARGO_INCREMENTAL="${CARGO_INCREMENTAL:-0}"
export CODETERM_RELEASE_FEATURES="${CODETERM_RELEASE_FEATURES:-embeddings}"
ARM_DMG="src-tauri/target/release/bundle/dmg/CodeTerm_${VER}_aarch64.dmg"

echo "=== build arm64 ===" >&2
"$HELPERS/build-mac.sh" arm64 >&2

echo "=== notarize+staple arm64 (background) ===" >&2
"$HELPERS/notarize-dmg.sh" "$ARM_DMG" >&2 &
ARM_NOTARY_PID=$!

X86_RC=0
"$HELPERS/mac-bundle.sh" "$VER" x86_64 "$WORK" >/dev/null || X86_RC=$?

ARM_RC=0
wait "$ARM_NOTARY_PID" || ARM_RC=$?
[ "$X86_RC" -eq 0 ] || { echo "x86_64 macOS pipeline failed (rc=$X86_RC)" >&2; exit "$X86_RC"; }
[ "$ARM_RC" -eq 0 ] || { echo "arm64 macOS notarization failed (rc=$ARM_RC)" >&2; exit "$ARM_RC"; }

echo "=== pack updater bundle aarch64 ===" >&2
"$HELPERS/pack-mac-updater.sh" "$VER" aarch64 "$WORK" >/dev/null
echo "=== macOS overlap complete ===" >&2
