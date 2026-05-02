#!/usr/bin/env bash
# Build two CodeTerm versions for the auto-update test rig.
# Outputs land under tests/fixtures/auto-update/dist/.
#
# DO NOT run this in CI without thinking first — it produces release-mode
# Tauri bundles, which take 5-15 minutes per pass and pull in the full
# native toolchain. Intended for manual local runs by developers.
#
# Usage:
#   tests/fixtures/auto-update/build-fixture.sh        # build both versions
#   FIXTURE_OLD_VERSION=0.0.1-test \
#   FIXTURE_NEW_VERSION=0.0.2-test \
#       tests/fixtures/auto-update/build-fixture.sh
#
# Reads/writes:
#   - src-tauri/tauri.conf.json  (version field — restored on exit)
#   - tests/fixtures/auto-update/dist/*  (artifacts)
#   - tests/fixtures/auto-update/manifest.json  (regenerated with real signatures)

set -euo pipefail

OLD_VERSION="${FIXTURE_OLD_VERSION:-0.0.1-test}"
NEW_VERSION="${FIXTURE_NEW_VERSION:-0.0.2-test}"

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
FIXTURE_DIR="$REPO_ROOT/tests/fixtures/auto-update"
DIST_DIR="$FIXTURE_DIR/dist"
TAURI_CONF="$REPO_ROOT/src-tauri/tauri.conf.json"
ORIG_VERSION="$(grep -E '"version":\s*"' "$TAURI_CONF" | head -1 | sed -E 's/.*"version":\s*"([^"]+)".*/\1/')"

mkdir -p "$DIST_DIR"

restore_conf() {
  if [[ -n "${ORIG_VERSION:-}" ]]; then
    sed -i.bak "s/\"version\": \"$NEW_VERSION\"/\"version\": \"$ORIG_VERSION\"/" "$TAURI_CONF" || true
    sed -i.bak "s/\"version\": \"$OLD_VERSION\"/\"version\": \"$ORIG_VERSION\"/" "$TAURI_CONF" || true
    rm -f "${TAURI_CONF}.bak"
  fi
}
trap restore_conf EXIT

build_one() {
  local version="$1"
  echo "[build-fixture] building CodeTerm $version"
  sed -i.bak -E "s/\"version\":\s*\"[^\"]+\"/\"version\": \"$version\"/" "$TAURI_CONF"
  rm -f "${TAURI_CONF}.bak"

  case "$(uname -s)" in
    MINGW*|MSYS*|CYGWIN*)
      bun run tauri build --features dev-updater-allowlocal -- --target x86_64-pc-windows-msvc
      cp "$REPO_ROOT/src-tauri/target/release/bundle/msi/"*.msi.zip "$DIST_DIR/codeterm-${version}-x64.msi.zip"
      cp "$REPO_ROOT/src-tauri/target/release/bundle/msi/"*.msi.zip.sig "$DIST_DIR/codeterm-${version}-x64.msi.zip.sig"
      ;;
    Darwin)
      local arch
      arch="$(uname -m)"
      bun run tauri build --features dev-updater-allowlocal
      cp "$REPO_ROOT/src-tauri/target/release/bundle/macos/"*.app.tar.gz "$DIST_DIR/codeterm-${version}-${arch}.app.tar.gz"
      cp "$REPO_ROOT/src-tauri/target/release/bundle/macos/"*.app.tar.gz.sig "$DIST_DIR/codeterm-${version}-${arch}.app.tar.gz.sig"
      ;;
    *)
      echo "[build-fixture] unsupported OS: $(uname -s)" >&2
      exit 1
      ;;
  esac
}

build_one "$OLD_VERSION"
build_one "$NEW_VERSION"

echo "[build-fixture] regenerating manifest with real signatures"
python3 "$FIXTURE_DIR/regenerate-manifest.py" \
  --version "$NEW_VERSION" \
  --dist "$DIST_DIR" \
  --output "$FIXTURE_DIR/manifest.json"

echo "[build-fixture] done. Serve with:"
echo "  cd $FIXTURE_DIR && python3 -m http.server 8080"
echo "Then run an OLD-version CodeTerm with:"
echo "  codeterm update --apply --manifest-url http://127.0.0.1:8080/manifest.json"
