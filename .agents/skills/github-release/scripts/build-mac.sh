#!/usr/bin/env bash
# Build + sign a notarization-ready macOS .app for one architecture.
# Wraps the easy-to-forget bits:
#   - sources ~/.claude/.secrets/apple-notary.env so Tauri picks up
#     APPLE_SIGNING_IDENTITY instead of ad-hoc-signing the inner binaries
#   - kills any running dev (which would fight for the cargo target dir)
#   - verifies the resulting .app's Authority + hardened-runtime flag before
#     handing off to notarize
#
# Usage: build-mac.sh arm64        (= pnpm tauri build)
#        build-mac.sh x86_64       (= scripts/release-macos-x86_64.sh)

set -euo pipefail

ARCH="${1:-}"
case "$ARCH" in
  arm64|aarch64) ARCH=arm64 ;;
  x86_64|x64)    ARCH=x86_64 ;;
  "") echo "usage: build-mac.sh arm64|x86_64" >&2; exit 2 ;;
  *) echo "unknown arch: $ARCH (expected arm64 or x86_64)" >&2; exit 2 ;;
esac

REPO_ROOT="$(cd "$(dirname "$0")/../../../.." && pwd)"
cd "$REPO_ROOT"

SECRETS="$HOME/.claude/.secrets/apple-notary.env"
[ -f "$SECRETS" ] || { echo "ERROR: $SECRETS not found — restore from BW secret codeterm-apple-notary-env" >&2; exit 1; }
set -a; source "$SECRETS"; set +a
[ -n "${APPLE_SIGNING_IDENTITY:-}" ] || { echo "ERROR: APPLE_SIGNING_IDENTITY not in $SECRETS" >&2; exit 1; }

# Killing the running dev avoids it fighting for the cargo target dir — but ONLY matters for a
# manual build from the working checkout. Under CI the build runs in the runner's own isolated
# checkout (separate target dir), so killing dev there just nukes the user's live dev session for
# nothing. Skip it on CI.
if [ -n "${GITHUB_ACTIONS:-}" ] || [ -n "${ACTIONS_RUNNER_SVC:-}" ] || [ -n "${CODETERM_BUILD_KEEP_DEV:-}" ]; then
  echo "CI/keep-dev mode — not killing the running dev (isolated checkout)"
else
  ./build.sh kill
fi

if [ "$ARCH" = "arm64" ]; then
  rm -rf src-tauri/target/release/bundle
  ./build.sh release-inputs
  pnpm tauri build --config src-tauri/tauri.conf.json
  APP=src-tauri/target/release/bundle/macos/CodeTerm.app
else
  rm -rf src-tauri/target/x86_64-apple-darwin/release/bundle
  bash scripts/release-macos-x86_64.sh
  APP=src-tauri/target/x86_64-apple-darwin/release/bundle/macos/CodeTerm.app
fi

# Verify the inner binary was signed by Developer ID, not the linker's ad-hoc.
# If this check fails, the notarize step would reject with "signature is invalid
# / no secure timestamp / hardened runtime not enabled" — better to catch here.
echo "--- verifying $APP/Contents/MacOS/codeterm signature ---"
INFO=$(codesign -dv --verbose=4 "$APP/Contents/MacOS/codeterm" 2>&1)
echo "$INFO" | grep -E "Authority|flags" | head -3
echo "$INFO" | grep -q "Authority=Developer ID Application" \
  || { echo "FAIL: not signed with Developer ID" >&2; exit 1; }
echo "$INFO" | grep -q "flags=0x10000(runtime)" \
  || { echo "FAIL: hardened runtime not enabled (flags must include 0x10000)" >&2; exit 1; }
echo "OK: Developer ID + hardened runtime"

# Verify arch of BOTH inner binaries matches the requested build target.
EXPECT_ARCH=$([ "$ARCH" = "arm64" ] && echo "arm64" || echo "x86_64")
echo "--- verifying CLI + GUI arch matches $EXPECT_ARCH ---"
file "$APP/Contents/MacOS/codeterm"        | grep -q "$EXPECT_ARCH" \
  || { echo "FAIL: $APP/Contents/MacOS/codeterm is not $EXPECT_ARCH: $(file "$APP/Contents/MacOS/codeterm")" >&2; exit 1; }
file "$APP/Contents/Resources/cli/codeterm" | grep -q "$EXPECT_ARCH" \
  || { echo "FAIL: $APP/Contents/Resources/cli/codeterm is not $EXPECT_ARCH: $(file "$APP/Contents/Resources/cli/codeterm")" >&2; exit 1; }
echo "OK: both binaries are $EXPECT_ARCH"
