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
LIBCXX_DIR="${CODETERM_MONTEREY_LIBCXX_DIR:-}"

find_libcxx_dir() {
  if [ -n "$LIBCXX_DIR" ]; then
    printf '%s\n' "$LIBCXX_DIR"
    return
  fi

  if [ "$(uname -m)" != "x86_64" ]; then
    echo "FATAL: set CODETERM_MONTEREY_LIBCXX_DIR to x86_64/universal LLVM libc++ dylibs" >&2
    echo "The arm64 Homebrew LLVM libc++ cannot be used for the x86_64 updater." >&2
    exit 1
  fi

  for dir in \
    /usr/local/opt/llvm/lib/c++ \
    /usr/local/opt/llvm/lib; do
    if [ -f "$dir/libc++.1.dylib" ]; then
      printf '%s\n' "$dir"
      return
    fi
  done

  echo "FATAL: CODETERM_MONTEREY_LIBCXX_DIR must point to x86_64 libc++ dylibs" >&2
  echo "Install or provide LLVM libc++ containing libc++.1.dylib and libc++abi.1.dylib." >&2
  exit 1
}

assert_x86_64_dylib() {
  local file="$1"
  file "$file" | grep -q x86_64 || {
    echo "FATAL: expected x86_64 dylib: $file -> $(file "$file")" >&2
    exit 1
  }
}

change_install_name_if_present() {
  local file="$1"
  local old="$2"
  local new="$3"
  if otool -L "$file" | awk '{print $1}' | grep -Fxq "$old"; then
    install_name_tool -change "$old" "$new" "$file"
  fi
}

# 1. Fetch ONNX Runtime 1.23.2 Intel mac dylib (once)
if [ ! -f "${ONNX_DIR}/lib/libonnxruntime.${ONNX_VER}.dylib" ]; then
  curl -sL -o "$ONNX_TGZ" \
    "https://github.com/microsoft/onnxruntime/releases/download/v${ONNX_VER}/onnxruntime-osx-x86_64-${ONNX_VER}.tgz"
  tar -xzf "$ONNX_TGZ" -C /tmp/
fi

# 2. Ensure target installed
rustup target add x86_64-apple-darwin >/dev/null

# 3. Stub the externalBin + resources globs so tauri_build's validation passes during step 3b.
mkdir -p src-tauri/bundled-embed src-tauri/bundled-cli
touch src-tauri/bundled-embed/codeterm-embed-x86_64-apple-darwin
HOST_TRIPLE=$(rustc -vV | awk '/^host:/ { print $2 }')
touch "src-tauri/bundled-embed/codeterm-embed-${HOST_TRIPLE}"
touch src-tauri/bundled-cli/codeterm

# 3b. Build the frontend and target-correct x86_64 sidecars.
CARGO_BUILD_TARGET=x86_64-apple-darwin ./build.sh release-inputs

# 4. Stage signed dylibs alongside the already bundled sidecars.
mkdir -p src-tauri/bundled-embed src-tauri/bundled-cli src-tauri/bundled-embed-libs
cp "${ONNX_DIR}/lib/libonnxruntime.${ONNX_VER}.dylib" src-tauri/bundled-embed-libs/libonnxruntime.dylib

LIBCXX_DIR="$(find_libcxx_dir)"
cp "$LIBCXX_DIR/libc++.1.dylib" src-tauri/bundled-embed-libs/libc++.1.dylib
if [ -f "$LIBCXX_DIR/libc++abi.1.dylib" ]; then
  cp "$LIBCXX_DIR/libc++abi.1.dylib" src-tauri/bundled-embed-libs/libc++abi.1.dylib
fi

assert_x86_64_dylib src-tauri/bundled-embed-libs/libonnxruntime.dylib
assert_x86_64_dylib src-tauri/bundled-embed-libs/libc++.1.dylib
if [ -f src-tauri/bundled-embed-libs/libc++abi.1.dylib ]; then
  assert_x86_64_dylib src-tauri/bundled-embed-libs/libc++abi.1.dylib
  change_install_name_if_present \
    src-tauri/bundled-embed-libs/libc++.1.dylib \
    /usr/lib/libc++abi.1.dylib \
    @loader_path/libc++abi.1.dylib
  change_install_name_if_present \
    src-tauri/bundled-embed-libs/libc++.1.dylib \
    @rpath/libc++abi.1.dylib \
    @loader_path/libc++abi.1.dylib
fi
change_install_name_if_present \
  src-tauri/bundled-embed-libs/libonnxruntime.dylib \
  /usr/lib/libc++.1.dylib \
  @loader_path/libc++.1.dylib
change_install_name_if_present \
  src-tauri/bundled-embed-libs/libonnxruntime.dylib \
  @rpath/libc++.1.dylib \
  @loader_path/libc++.1.dylib
otool -L src-tauri/bundled-embed-libs/libonnxruntime.dylib | grep -q '@loader_path/libc++.1.dylib' || {
  echo "FATAL: libonnxruntime.dylib still does not load bundled libc++" >&2
  otool -L src-tauri/bundled-embed-libs/libonnxruntime.dylib >&2
  exit 1
}

for f in src-tauri/bundled-embed/codeterm-embed-x86_64-apple-darwin \
         src-tauri/bundled-cli/codeterm \
         src-tauri/bundled-embed-libs/libonnxruntime.dylib \
         src-tauri/bundled-embed-libs/libc++.1.dylib \
         src-tauri/bundled-embed-libs/libc++abi.1.dylib; do
  [ -f "$f" ] || continue
  codesign --force --options runtime --timestamp --sign "$SIGN_ID" "$f"
done

# 5. Merge config: add bundled-embed-libs/* to resources (don't suppress beforeBuildCommand).
cat > /tmp/tauri.x86_64.conf.json <<'JSONEOF'
{
  "bundle": {
    "resources": {
      "../skills": "skills",
      "bundled-cli/*": "cli/",
      "bundled-embed-libs/*": "embed-libs/"
    }
  }
}
JSONEOF

# 6. Tauri build with cross-target + updater signing. CARGO_BUILD_TARGET forces every cargo
# invocation inside beforeBuildCommand to cross-compile; TAURI_SIGNING_PRIVATE_KEY makes Tauri
# emit the .app.tar.gz + .sig updater bundle alongside the DMG.
export CARGO_BUILD_TARGET=x86_64-apple-darwin
export TAURI_SIGNING_PRIVATE_KEY="$(cat ~/.tauri/codeterm.key)"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD=""
pnpm tauri build --target x86_64-apple-darwin --config /tmp/tauri.x86_64.conf.json
unset CARGO_BUILD_TARGET TAURI_SIGNING_PRIVATE_KEY TAURI_SIGNING_PRIVATE_KEY_PASSWORD

# 6b. Arch assertion: fail before notarize if CLI or GUI is wrong arch.
APP_CLI="src-tauri/target/x86_64-apple-darwin/release/bundle/macos/CodeTerm.app/Contents/Resources/cli/codeterm"
APP_GUI="src-tauri/target/x86_64-apple-darwin/release/bundle/macos/CodeTerm.app/Contents/MacOS/codeterm"
file "$APP_CLI" | grep -q x86_64 || { echo "FATAL: bundled CLI is not x86_64: $(file "$APP_CLI")"; exit 1; }
file "$APP_GUI" | grep -q x86_64 || { echo "FATAL: bundled GUI is not x86_64: $(file "$APP_GUI")"; exit 1; }
echo "OK: both bundled CLI and GUI are x86_64"

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
