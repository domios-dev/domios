#!/usr/bin/env bash
# Builds signed + notarized x86_64 macOS DMG with full embeddings (ONNX dylib bundled).
# Run from the codeterm repo root on an arm64 Mac.
set -euo pipefail

REPO="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO"

SECRETS="$HOME/.claude/.secrets/apple-notary.env"
set -a; source "$SECRETS"; set +a
SIGN_ID="Developer ID Application: ANDREY KOVALEV (JMMWQ9LWF7)"

ONNX_VER="1.23.2"
ONNX_DIR="/tmp/onnxruntime-osx-x86_64-${ONNX_VER}"
ONNX_TGZ="${ONNX_DIR}.tgz"

# 1. Fetch ONNX Runtime 1.23.2 Intel mac dylib (once)
if [ ! -f "${ONNX_DIR}/lib/libonnxruntime.${ONNX_VER}.dylib" ]; then
  curl -sL -o "$ONNX_TGZ" \
    "https://github.com/microsoft/onnxruntime/releases/download/v${ONNX_VER}/onnxruntime-osx-x86_64-${ONNX_VER}.tgz"
  tar -xzf "$ONNX_TGZ" -C /tmp/
fi

# 2. Ensure target installed
rustup target add x86_64-apple-darwin >/dev/null

# 3. Build x86_64 sidecar + main bin + CLI
cargo build --release --target x86_64-apple-darwin --manifest-path src-tauri/Cargo.toml --bin codeterm-embed
cargo build --release --target x86_64-apple-darwin --manifest-path cli/Cargo.toml

# 4. Stage signed binaries and dylib
mkdir -p src-tauri/bundled-embed src-tauri/bundled-cli src-tauri/bundled-embed-libs
cp src-tauri/target/x86_64-apple-darwin/release/codeterm-embed src-tauri/bundled-embed/codeterm-embed-x86_64-apple-darwin
# Temporarily swap CLI to x86_64 (remember to restore arm64 after)
cp cli/target/x86_64-apple-darwin/release/codeterm src-tauri/bundled-cli/codeterm
cp "${ONNX_DIR}/lib/libonnxruntime.${ONNX_VER}.dylib" src-tauri/bundled-embed-libs/libonnxruntime.dylib

for f in src-tauri/bundled-embed/codeterm-embed-x86_64-apple-darwin \
         src-tauri/bundled-cli/codeterm \
         src-tauri/bundled-embed-libs/libonnxruntime.dylib; do
  codesign --force --options runtime --timestamp --sign "$SIGN_ID" "$f"
done

# 5. Write merge config
cat > /tmp/tauri.x86_64.conf.json <<'JSONEOF'
{
  "build": {
    "beforeDevCommand": "",
    "beforeBuildCommand": ""
  },
  "bundle": {
    "resources": {
      "../skills": "skills",
      "bundled-cli/*": "cli/",
      "bundled-embed-libs/*": "embed-libs/"
    }
  }
}
JSONEOF

# 6. Full Tauri build → notarized .app + DMG wrapper (signed, not yet DMG-notarized)
pnpm tauri build --target x86_64-apple-darwin --config /tmp/tauri.x86_64.conf.json

DMG="src-tauri/target/x86_64-apple-darwin/release/bundle/dmg/CodeTerm_$(jq -r .version package.json)_x64.dmg"

# 7. Notarize the DMG itself (Tauri skips this)
xcrun notarytool submit "$DMG" \
  --apple-id "$APPLE_ID" --team-id "$APPLE_TEAM_ID" \
  --password "$APPLE_APP_SPECIFIC_PASSWORD" --wait

# 8. Staple + verify
xcrun stapler staple "$DMG"
spctl --assess --type open --context context:primary-signature --verbose "$DMG"
xcrun stapler validate "$DMG"

# 9. Restore arm64 CLI to bundled-cli so future arm64 builds aren't broken
cargo build --release --manifest-path cli/Cargo.toml >/dev/null
cp cli/target/release/codeterm src-tauri/bundled-cli/codeterm
codesign --force --options runtime --timestamp --sign "$SIGN_ID" src-tauri/bundled-cli/codeterm

echo
echo "Done. DMG: $DMG"
echo "Upload with: gh release upload v<ver> \"$DMG\" --clobber --repo rollacode/codeterm"
