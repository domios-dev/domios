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
if [ "$TAG" != source ]; then git checkout -q "tags/${TAG}"; fi
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-/src/src-tauri/target}"
echo "HEAD=$(git rev-parse HEAD)  bundles=$BUNDLES  target=${CARGO_BUILD_TARGET:-native}"
source scripts/cache-policy.sh
export CODETERM_SCCACHE_CREDENTIAL_SOURCE=runner
# The container's server dies with it, so a local cache here is already job-local.
if ! ct_ci_store_reachable; then
  echo "WARNING: private store is unreachable; this container compiles with its own local sccache"
  export CODETERM_SCCACHE_STORAGE=local CODETERM_SCCACHE_JOB_LOCAL=1
fi
CODETERM_SCCACHE_VERIFY_ONLY=0 ct_configure_sccache
ct_sccache_verify
export CODETERM_SCCACHE_VERIFY_ONLY=1
trap 'ct_sccache_stats' EXIT

# Fast compile gate: cargo check only, no frontend/bundle/sign. Catches
# target-gated compile breaks in ~1 min. Stub tauri-build resources so its glob
# validation passes without a real dist/cli.
if [ "${CHECK:-0}" = "1" ]; then
  mkdir -p dist src-tauri/bundled-cli
  touch dist/index.html src-tauri/bundled-cli/codeterm
  cargo check --manifest-path src-tauri/Cargo.toml ${CARGO_BUILD_TARGET:+--target "$CARGO_BUILD_TARGET"}
  echo "CHECK_LINUX_DONE tag=$TAG target=${CARGO_BUILD_TARGET:-native}"
  exit 0
fi

corepack prepare pnpm@10.32.1 --activate >/dev/null 2>&1 || true
# Cross builds need CLI resource stubs pre-created (tauri-build glob validation runs
# during cargo builds, before bundle scripts stage the real cross binaries).
if [ -n "${CARGO_BUILD_TARGET:-}" ]; then
  mkdir -p src-tauri/bundled-cli
  touch src-tauri/bundled-cli/codeterm
fi
pnpm install --frozen-lockfile
TARGET="${CARGO_BUILD_TARGET:-$(rustc -vV | awk '/^host:/ { print $2 }')}"
CODETERM_RELEASE_COMMON_STAGE=1 ./build.sh release-inputs --for-bundle
pnpm tauri build --no-bundle ${CARGO_BUILD_TARGET:+--target "$CARGO_BUILD_TARGET"} \
  ${CODETERM_RELEASE_FEATURES:+--features "$CODETERM_RELEASE_FEATURES"} \
  --config '{"bundle":{"createUpdaterArtifacts":false,"resources":[],"externalBin":[]}}'
CLIENT="src-tauri/target/${CARGO_BUILD_TARGET:+$CARGO_BUILD_TARGET/}release/domios-app"
node scripts/release-stage.mjs stage-full --target "$TARGET" --source-sha "$(git rev-parse HEAD)" \
  --cargo-log "release-artifacts/build-logs/$TARGET/server.jsonl" \
  --tree-log "release-artifacts/build-logs/$TARGET/closure.txt" --client-file "$CLIENT"
pnpm tauri bundle --no-binary-patching ${CARGO_BUILD_TARGET:+--target "$CARGO_BUILD_TARGET"} \
  ${CODETERM_RELEASE_FEATURES:+--features "$CODETERM_RELEASE_FEATURES"} --bundles "$BUNDLES" \
  --config '{"bundle":{"createUpdaterArtifacts":false}}'

mkdir -p /work/out
SUB="${CARGO_BUILD_TARGET:+$CARGO_BUILD_TARGET/}"
find "src-tauri/target/${SUB}release/bundle" -maxdepth 2 -type f \
  \( -name '*.deb' -o -name '*.rpm' -o -name '*.AppImage' \) -exec cp -v {} /work/out/ \;
mapfile -t DEBS < <(find "src-tauri/target/${SUB}release/bundle/deb" -maxdepth 1 -type f -name '*.deb')
[ "${#DEBS[@]}" -eq 1 ] || { echo "Expected exactly one full Debian package" >&2; exit 1; }
: "${RELEASE_TAG:?Release tag is required for common-stage receipt}"
node scripts/release-stage.mjs finalize --target "$TARGET" --tag "$RELEASE_TAG" \
  --version "${RELEASE_TAG#v}" --source-sha "$(git rev-parse HEAD)" \
  --cargo-log "release-artifacts/build-logs/$TARGET/server.jsonl" \
  --tree-log "release-artifacts/build-logs/$TARGET/closure.txt" \
  --bundle "${DEBS[0]}" --output-dir /work/out
cp "release-artifacts/common-stage/$TARGET/headless-stage.json" "/work/out/headless-stage-$TARGET.json"
node scripts/release-bundle-smoke.mjs linux /work/out "$(git rev-parse HEAD)"
ls -la /work/out/
echo "BUILD_LINUX_DONE tag=$TAG"
