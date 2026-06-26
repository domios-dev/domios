#!/usr/bin/env bash
# CodeTerm local build machine — orchestrator.
#
# Topology: 3 independent build engines, each builds ONE arch per wave, so 3 artifacts
# build concurrently per wave; two waves cover all 6 targets.
#
#   ENGINE        WHERE                 arm64 wave            x86_64 wave
#   mac host      native macOS          macOS aarch64 dmg     macOS x86_64 dmg
#   docker        baked linux images    linux arm64 .deb      linux x86_64 .deb (cross)
#   windows VM    ssh into QEMU/peer     windows arm64 .exe    windows x86_64 .exe
#
# Builds are CLEAN (fresh git clone of the tag inside each engine; no cargo target cache).
# The heavy toolchain/deps are pre-baked into the Docker images and the VM, so per-build
# startup is fast.
#
# Prereqs:
#   - docker images:  codeterm-build-linux-arm64 / codeterm-build-linux-x64
#       (build once:  docker build -t codeterm-build-linux-arm64 -f docker/Dockerfile.linux-arm64 .
#                     docker build -t codeterm-build-linux-x64   -f docker/Dockerfile.linux-x64   . )
#   - macOS signing: ~/.claude/.secrets/apple-notary.env + Developer ID in keychain + notary profile
#   - windows VM:    WIN_SSH / WIN_PORT / WIN_SSH_KEY env (see build-windows.sh)
#   - minisign updater key on the Mac: ~/.tauri/codeterm.key (updater .sig signed on the Mac)
#
# Usage:  build-all.sh vX.Y.Z
set -euo pipefail
TAG="${1:?usage: build-all.sh vX.Y.Z}"; VER="${TAG#v}"
REPO="$(cd "$(dirname "$0")/../../.." && pwd)"
BM="$REPO/.codeterm/build-machine"
HELPERS="$REPO/.agents/skills/github-release/scripts"
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

# Pre-warm the Windows VM so it cold-boots in parallel with wave-1's mac/linux compiles instead of
# starting from cold under full CPU contention when the windows engine first reaches it (that
# starves the boot and the stability gate times out). build-windows.sh's gate waits for it.
VM_DIR="$HOME/VMs/codeterm-win11-arm"
if [ -x "$VM_DIR/vm.sh" ] && ! pgrep -f qemu-system-aarch64 >/dev/null 2>&1; then
  echo "### pre-warming Windows VM (background) so it is ready by the time the windows engine needs it"
  ( cd "$VM_DIR" && nohup ./vm.sh run >/tmp/win-vm-boot.log 2>&1 & ) 2>/dev/null || true
fi

# Short engine name from its command, so per-engine logs are attributable.
engine_name() {
  case "$1" in
    *mac-bundle*)    echo "macos" ;;
    *build-linux*)   echo "linux" ;;
    *build-windows*) echo "windows" ;;
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

DOCKER="docker run --rm -v $LWORK:/work -v /tmp/ct-cargo-registry:/usr/local/cargo/registry -v $BM/scripts/build-linux.sh:/build-linux.sh -e CARGO_BUILD_JOBS=2 -w /work"

# The mac engine builds the GUI/CLI from THIS checkout; a fresh CI checkout has no
# node_modules, so install once up front (linux/windows engines install inside themselves).
echo "### pnpm install (mac engine, $REPO)"
( cd "$REPO" && { corepack enable >/dev/null 2>&1 || true; }; pnpm install )

echo "### cleaning mac release target dirs (avoid interrupted prior release state)"
rm -rf "$REPO/src-tauri/target/release" \
       "$REPO/src-tauri/target/x86_64-apple-darwin/release"

rc=0

# arm64 wave
run_wave arm64 \
  "$HELPERS/mac-bundle.sh $VER arm64 $ART" \
  "$DOCKER codeterm-build-linux-arm64 bash /build-linux.sh $TAG deb,appimage; cp $LWORK/out/*arm64.deb $ART/ 2>/dev/null; cp $LWORK/out/*aarch64.AppImage $ART/ 2>/dev/null" \
  "OUT=$ART bash $BM/scripts/build-windows.sh $TAG aarch64" || rc=1

# x86_64 wave
run_wave x86_64 \
  "$HELPERS/mac-bundle.sh $VER x86_64 $ART" \
  "$DOCKER codeterm-build-linux-x64 bash /build-linux.sh $TAG deb; cp $LWORK/out/*amd64.deb $ART/ 2>/dev/null" \
  "OUT=$ART bash $BM/scripts/build-windows.sh $TAG x86_64" || rc=1

# Collect mac DMGs into the artifacts dir (mac-bundle leaves them in target/).
cp -f "$REPO/src-tauri/target/release/bundle/dmg/CodeTerm_${VER}_aarch64.dmg" "$ART/" 2>/dev/null || true
cp -f "$REPO/src-tauri/target/x86_64-apple-darwin/release/bundle/dmg/CodeTerm_${VER}_x64.dmg" "$ART/" 2>/dev/null || true

# Sign updater-capable bundles with the minisign updater key (key stays on the Mac).
for bundle in "$ART"/CodeTerm_${VER}_*-setup.exe "$ART"/CodeTerm_${VER}_*.AppImage; do
  [ -f "$bundle" ] && ( cd "$REPO" && set -a; . ~/.claude/.secrets/apple-notary.env; set +a; pnpm tauri signer sign "$bundle" )
done

echo "### artifacts in $ART"; ls -la "$ART"
[ "$rc" -eq 0 ] || { echo "::error::one or more release build waves failed"; exit "$rc"; }
echo "BUILD_ALL_DONE $TAG"
