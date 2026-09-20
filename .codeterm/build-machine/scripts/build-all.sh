#!/usr/bin/env bash
# CodeTerm local build machine — orchestrator (macOS + Linux only).
#
# Topology: the Mac pipeline builds both macOS arches in one overlap wave; Docker builds
# optional Linux arm64 alongside it. x86_64 Linux runs on the ROG/WSL runner.
#
#   ENGINE        WHERE                 overlap wave
#   mac host      native macOS          macOS aarch64 + x86_64 DMGs
#   docker        baked Linux image     linux arm64 .deb+.AppImage (optional)
#
# Other jobs in the single release.yml run use the ROG self-hosted runners:
#   - Windows x64+arm64 -> release-windows.yml (label windows-build)
#   - x86_64 Linux .deb+.AppImage -> release-linux.yml (label linux-build, native in WSL2)
# x86_64 Linux moved off this Mac because the AppImage can't be cross-built on ARM.
#
# Builds are CLEAN (fresh git clone of the tag inside each engine; no cargo target cache).
# The heavy toolchain/deps are pre-baked into the Docker images, so per-build startup is fast.
#
# Prereqs:
#   - docker images:  codeterm-build-linux-arm64 / codeterm-build-linux-x64
#       (build once:  docker build -t codeterm-build-linux-arm64 -f docker/Dockerfile.linux-arm64 .
#                     docker build -t codeterm-build-linux-x64   -f docker/Dockerfile.linux-x64   . )
#   - macOS signing: ~/.claude/.secrets/apple-notary.env + Developer ID in keychain + notary profile
#   - minisign updater key on the Mac: ~/.tauri/codeterm.key (updater .sig signed on the Mac)
#
# Usage:  build-all.sh vX.Y.Z
set -euo pipefail
TAG="${1:?usage: build-all.sh vX.Y.Z}"; VER="${TAG#v}"
REPO="$(cd "$(dirname "$0")/../../.." && pwd)"
BM="$REPO/.codeterm/build-machine"
ART="${ART:-/tmp/codeterm-${VER}-artifacts}"
LWORK=/tmp/ct-linux-build
rm -rf "$ART" "$LWORK"
mkdir -p "$ART" "$LWORK"

cleanup_children() {
  local pids
  pids="$(jobs -pr || true)"
  [ -n "$pids" ] || return 0
  for pid in $pids; do
    pkill -TERM -P "$pid" 2>/dev/null || true
    kill -TERM "$pid" 2>/dev/null || true
  done
  sleep 2
  for pid in $pids; do
    pkill -KILL -P "$pid" 2>/dev/null || true
    kill -KILL "$pid" 2>/dev/null || true
  done
}
trap 'cleanup_children' INT TERM

echo "### creating auth-free source bundle for $TAG"
git -C "$REPO" bundle create "/tmp/codeterm-${TAG}.bundle" "$TAG"
cp "/tmp/codeterm-${TAG}.bundle" "$LWORK/${TAG}.bundle"

# Short engine name from its command, so per-engine logs are attributable.
engine_name() {
  case "$1" in
    *mac-bundle*|*mac-overlap*) echo "macos" ;;
    *build-linux*)   echo "linux" ;;
    *)               echo "engine" ;;
  esac
}

# Run N engine commands concurrently, wait for all (a failing engine doesn't abort the wave).
# Each engine's output is captured then replayed into the Actions log inside a foldable
# ::group:: with status + duration, so the build is no longer a black box.
run_wave() {
  local label="$1"; shift
  local n=$#
  echo "### WAVE $label : launching $n engines concurrently at $(date -u +%H:%M:%SZ)"
  local pids=() names=() logs=() starts=() i=0 rc=0
  for c in "$@"; do
    local name log
    name="$(engine_name "$c")-$label"
    log="$ART/.build-$label-$i-$name.log"
    names+=("$name"); logs+=("$log"); starts+=("$(date +%s)")
    echo "  ▶ [$name] launched ($(date -u +%H:%M:%SZ))"
    ( eval "$c" ) >"$log" 2>&1 &
    pids+=($!)
    i=$((i + 1))
  done
  local idx
  for idx in "${!pids[@]}"; do
    local prc=0
    wait "${pids[$idx]}" || prc=1
    local dur=$(( $(date +%s) - ${starts[$idx]} ))
    local st="✅ ok"
    [ "$prc" -eq 0 ] || { st="❌ FAILED (rc=$prc)"; rc=1; }
    echo "::group::[$label] ${names[$idx]} — $st in ${dur}s"
    cat "${logs[$idx]}" 2>/dev/null || true
    echo "::endgroup::"
    echo "  ◀ [${names[$idx]}] $st in ${dur}s"
  done
  echo "### WAVE $label done (rc=$rc) at $(date -u +%H:%M:%SZ)"
  return "$rc"
}

DOCKER="docker run --rm -v $LWORK:/work -v /tmp/ct-cargo-registry:/usr/local/cargo/registry -v $BM/scripts/build-linux.sh:/build-linux.sh -e CARGO_BUILD_JOBS=2 -e CARGO_TARGET_DIR=/src/src-tauri/target -e CARGO_INCREMENTAL=0 -e CODETERM_RELEASE_FEATURES=embeddings -w /work"

export CARGO_TARGET_DIR="$REPO/src-tauri/target"
export CARGO_INCREMENTAL=0
export CODETERM_RELEASE_FEATURES=embeddings

# The mac engine builds the GUI/CLI from THIS checkout; a fresh CI checkout has no
# node_modules, so install once up front (linux/windows engines install inside themselves).
echo "### pnpm install (mac engine, $REPO)"
( cd "$REPO" && { corepack enable >/dev/null 2>&1 || true; }; pnpm install )

echo "### cleaning mac release target dirs (avoid interrupted prior release state)"
rm -rf "$REPO/src-tauri/target/release" \
       "$REPO/src-tauri/target/x86_64-apple-darwin/release"

rc=0

# arm64 wave. Linux lane disabled for v1.9.5 (user decision, 2026-08-18):
# set CT_BUILD_LINUX=1 to re-enable the docker arm64 Linux engine.
if [ "${CT_BUILD_LINUX:-0}" = "1" ]; then
  run_wave arm64 \
    "$BM/scripts/mac-overlap.sh $VER $ART" \
    "$DOCKER codeterm-build-linux-arm64 bash /build-linux.sh $TAG deb,appimage; cp $LWORK/out/*arm64.deb $ART/ 2>/dev/null; cp $LWORK/out/*aarch64.AppImage $ART/ 2>/dev/null" || rc=1
else
  echo "[build-all] Linux arm64 engine skipped (CT_BUILD_LINUX!=1)"
  run_wave arm64 \
    "$BM/scripts/mac-overlap.sh $VER $ART" \
    "true" || rc=1
fi

# Collect mac DMGs into the artifacts dir (mac-bundle leaves them in target/).
cp -f "$REPO/src-tauri/target/release/bundle/dmg/CodeTerm_${VER}_aarch64.dmg" "$ART/" 2>/dev/null || true
cp -f "$REPO/src-tauri/target/x86_64-apple-darwin/release/bundle/dmg/CodeTerm_${VER}_x64.dmg" "$ART/" 2>/dev/null || true

# Sign updater-capable bundles with the minisign updater key (key stays on the Mac).
# Windows installers are signed by the Windows reusable job, not here.
for bundle in "$ART"/CodeTerm_${VER}_*.AppImage; do
  [ -f "$bundle" ] && ( cd "$REPO" && set -a; . ~/.claude/.secrets/apple-notary.env; set +a; pnpm tauri signer sign "$bundle" )
done

echo "### artifacts in $ART"; ls -la "$ART"

# Post-build cache reclaim — artifacts are already collected in $ART, so the build
# intermediates are pure garbage. Release builds otherwise leave the mac release bundle,
# the linux $LWORK target and docker buildx cache to grow unbounded on the builder
# (cargo has no target GC — this is the disk-bloat class). Best-effort; never touches $ART.
echo "### post-build cleanup (reclaim build caches; artifacts safe in $ART)"
rm -rf "$LWORK" 2>/dev/null || true
rm -rf "$REPO/src-tauri/target/release/bundle" \
       "$REPO/src-tauri/target/x86_64-apple-darwin/release/bundle" 2>/dev/null || true
if command -v docker >/dev/null 2>&1 && docker info >/dev/null 2>&1; then
  docker buildx prune -af >/dev/null 2>&1 || true
  docker container prune -f  >/dev/null 2>&1 || true
  docker image prune -f      >/dev/null 2>&1 || true
  # Drop stale codeterm-build-linux-* images; KEEP the baked arm64 env this
  # builder still uses (codeterm-build-linux-arm64). x86_64 moved to ubunta.
  docker images --format '{{.Repository}}:{{.Tag}}' 2>/dev/null \
    | grep '^codeterm-build-linux-' | grep -v '^codeterm-build-linux-arm64:' \
    | xargs -r docker rmi -f >/dev/null 2>&1 || true
fi
df -h / | awk 'NR==2{print "### builder free after cleanup:", $4}'

[ "$rc" -eq 0 ] || { echo "::error::one or more release build waves failed"; exit "$rc"; }
echo "BUILD_ALL_DONE $TAG"
