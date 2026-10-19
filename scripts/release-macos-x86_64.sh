#!/usr/bin/env bash
# Caller: scripts check-dev-resource-preflight.sh, tests/release-job-graph.test.mjs; skill build-machine, github-release
# Builds a signed x86_64 macOS DMG; mac-bundle.sh owns notarization.
# Run from the codeterm repo root on an arm64 Mac.
set -euo pipefail

REPO="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO"

: "${APPLE_SIGNING_IDENTITY:?Provide APPLE_SIGNING_IDENTITY}"
: "${RELEASE_KEYCHAIN:?Import the job signing keychain first}"

rustup target add x86_64-apple-darwin >/dev/null
mkdir -p src-tauri/bundled-cli
touch src-tauri/bundled-cli/codeterm
CARGO_BUILD_TARGET=x86_64-apple-darwin \
  CODETERM_RELEASE_COMMON_STAGE=1 \
  CODETERM_RELEASE_FEATURES="${CODETERM_RELEASE_FEATURES:-embeddings}" \
  ./build.sh release-inputs --for-bundle
export CARGO_BUILD_TARGET=x86_64-apple-darwin
: "${TAURI_SIGNING_PRIVATE_KEY_PATH:?Prepare the job updater key first}"
pnpm tauri build --no-bundle --target x86_64-apple-darwin \
  --features "${CODETERM_RELEASE_FEATURES:-embeddings}"
node scripts/release-stage.mjs stage-full --target x86_64-apple-darwin \
  --source-sha "$(git rev-parse HEAD)" \
  --cargo-log release-artifacts/build-logs/x86_64-apple-darwin/server.jsonl \
  --tree-log release-artifacts/build-logs/x86_64-apple-darwin/closure.txt \
  --client-file src-tauri/target/x86_64-apple-darwin/release/domios
pnpm tauri bundle --no-binary-patching --target x86_64-apple-darwin --features "${CODETERM_RELEASE_FEATURES:-embeddings}" --bundles app,dmg
unset CARGO_BUILD_TARGET

# 6b. Arch assertion: fail before notarize if CLI or GUI is wrong arch.
APP_CLI="src-tauri/target/x86_64-apple-darwin/release/bundle/macos/CodeTerm.app/Contents/Resources/cli/codeterm"
APP_GUI="src-tauri/target/x86_64-apple-darwin/release/bundle/macos/CodeTerm.app/Contents/MacOS/domios"
file "$APP_CLI" | grep -q x86_64 || { echo "FATAL: bundled CLI is not x86_64: $(file "$APP_CLI")"; exit 1; }
file "$APP_GUI" | grep -q x86_64 || { echo "FATAL: bundled GUI is not x86_64: $(file "$APP_GUI")"; exit 1; }
echo "OK: both bundled CLI and GUI are x86_64"

DMG="src-tauri/target/x86_64-apple-darwin/release/bundle/dmg/CodeTerm_$(jq -r .version package.json)_x64.dmg"

echo
echo "Done. DMG: $DMG"
echo "Upload with: gh release upload v<ver> \"$DMG\" --clobber --repo rollacode/codeterm"
