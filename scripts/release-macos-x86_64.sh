#!/usr/bin/env bash
# Caller: scripts check-dev-resource-preflight.sh, tests/release-job-graph.test.mjs; skill build-machine, github-release
# Builds a signed x86_64 macOS DMG; mac-bundle.sh owns notarization.
# Run from the codeterm repo root on an arm64 Mac.
set -euo pipefail

REPO="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO"

: "${APPLE_SIGNING_IDENTITY:?Provide APPLE_SIGNING_IDENTITY}"
: "${RELEASE_KEYCHAIN:?Import the job signing keychain first}"
SIGN_ID="$APPLE_SIGNING_IDENTITY"

rustup target add x86_64-apple-darwin >/dev/null
mkdir -p src-tauri/bundled-cli
touch src-tauri/bundled-cli/codeterm
CARGO_BUILD_TARGET=x86_64-apple-darwin \
  CODETERM_RELEASE_FEATURES="${CODETERM_RELEASE_FEATURES:-embeddings}" \
  ./build.sh release-inputs
codesign --force --options runtime --timestamp --keychain "$RELEASE_KEYCHAIN" --sign "$SIGN_ID" src-tauri/bundled-cli/codeterm

export CARGO_BUILD_TARGET=x86_64-apple-darwin
: "${TAURI_SIGNING_PRIVATE_KEY_PATH:?Prepare the job updater key first}"
pnpm tauri build --target x86_64-apple-darwin \
  --features "${CODETERM_RELEASE_FEATURES:-embeddings}"
unset CARGO_BUILD_TARGET

# 6b. Arch assertion: fail before notarize if CLI or GUI is wrong arch.
APP_CLI="src-tauri/target/x86_64-apple-darwin/release/bundle/macos/CodeTerm.app/Contents/Resources/cli/codeterm"
APP_GUI="src-tauri/target/x86_64-apple-darwin/release/bundle/macos/CodeTerm.app/Contents/MacOS/codeterm"
file "$APP_CLI" | grep -q x86_64 || { echo "FATAL: bundled CLI is not x86_64: $(file "$APP_CLI")"; exit 1; }
file "$APP_GUI" | grep -q x86_64 || { echo "FATAL: bundled GUI is not x86_64: $(file "$APP_GUI")"; exit 1; }
echo "OK: both bundled CLI and GUI are x86_64"

DMG="src-tauri/target/x86_64-apple-darwin/release/bundle/dmg/CodeTerm_$(jq -r .version package.json)_x64.dmg"

# 7. Restore arm64 CLI to bundled-cli so future arm64 builds aren't broken
CARGO_TARGET_DIR=cli/target cargo build --release --manifest-path cli/Cargo.toml >/dev/null
cp cli/target/release/codeterm src-tauri/bundled-cli/codeterm
codesign --force --options runtime --timestamp --keychain "$RELEASE_KEYCHAIN" --sign "$SIGN_ID" src-tauri/bundled-cli/codeterm

echo
echo "Done. DMG: $DMG"
echo "Upload with: gh release upload v<ver> \"$DMG\" --clobber --repo rollacode/codeterm"
