#!/usr/bin/env bash
# Thin in-container Linux build step. Runs inside a baked image (Dockerfile.linux-arm64
# or .linux-x64). Clean build: fresh clone of the tag, no cargo target cache.
#
# Mounts expected:
#   /work/<tag>.bundle   git bundle of the tag (auth-free source transfer)
#   /work/out            output dir (host-mounted) for collected artifacts
#
# Usage (inside container):  build-linux.sh <tag> [bundles]
#   bundles: comma list for `tauri build --bundles` (default: deb,appimage on arm64; deb on x64)
set -euo pipefail
TAG="${1:?usage: build-linux.sh <tag> [bundles]}"
BUNDLES="${2:-}"
if [ -z "$BUNDLES" ]; then
  if [ -n "${CARGO_BUILD_TARGET:-}" ]; then BUNDLES=deb; else BUNDLES=deb,appimage; fi
fi

rm -rf /src
git clone -q "/work/${TAG}.bundle" /src
cd /src
git checkout -q "tags/${TAG}"
echo "HEAD=$(git rev-parse HEAD)  bundles=$BUNDLES  target=${CARGO_BUILD_TARGET:-native}"

corepack prepare pnpm@latest --activate >/dev/null 2>&1 || true
# Cross builds need externalBin/resource stubs pre-created (tauri-build glob validation runs
# during the embed/cli cargo builds, before bundle scripts stage the real cross binaries).
if [ -n "${CARGO_BUILD_TARGET:-}" ]; then
  mkdir -p src-tauri/bundled-embed src-tauri/bundled-cli
  touch "src-tauri/bundled-embed/codeterm-embed-${CARGO_BUILD_TARGET}"
  touch "src-tauri/bundled-embed/codeterm-embed-$(rustc -vV | awk '/^host:/{print $2}')"
  touch src-tauri/bundled-cli/codeterm
fi
pnpm install
pnpm tauri build ${CARGO_BUILD_TARGET:+--target "$CARGO_BUILD_TARGET"} --bundles "$BUNDLES" \
  || echo "tauri build returned non-zero (updater-sign step has no key in-container; bundles built before it)"

mkdir -p /work/out
SUB="${CARGO_BUILD_TARGET:+$CARGO_BUILD_TARGET/}"
find "src-tauri/target/${SUB}release/bundle" -maxdepth 2 -type f \
  \( -name '*.deb' -o -name '*.rpm' -o -name '*.AppImage' \) -exec cp -v {} /work/out/ \;
ls -la /work/out/
echo "BUILD_LINUX_DONE tag=$TAG"
