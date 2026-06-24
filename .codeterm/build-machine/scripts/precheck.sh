#!/usr/bin/env bash
# Fast cross-platform compile gate. Runs `cargo check` for Linux (arm64 + x86_64
# in the baked containers) and macOS (arm64 + x86_64) BEFORE the heavy build, so
# a target-gated compile break (e.g. a cfg-scoped dependency) fails in ~1-2 min
# instead of after the full multi-platform bundle/sign/notarize.
#
# Windows is intentionally not pre-checked: its compile is VM-boot-bound, so a
# separate fast gate saves nothing — that platform is covered at build time.
#
# Usage:  precheck.sh vX.Y.Z
set -uo pipefail
TAG="${1:?usage: precheck.sh vX.Y.Z}"
REPO="$(cd "$(dirname "$0")/../../.." && pwd)"
BM="$REPO/.codeterm/build-machine"
LWORK=/tmp/ct-precheck
rm -rf "$LWORK"; mkdir -p "$LWORK"
git -C "$REPO" bundle create "$LWORK/${TAG}.bundle" "$TAG"

DOCKER="docker run --rm -v $LWORK:/work -v /tmp/ct-cargo-registry:/usr/local/cargo/registry -v $BM/scripts/build-linux.sh:/build-linux.sh -e CHECK=1 -w /work"

rc=0
run() {
  local label="$1"; shift
  local t0; t0=$(date +%s)
  echo "::group::precheck $label"
  if "$@"; then
    echo "  ✅ $label ok in $(( $(date +%s) - t0 ))s"
  else
    echo "::error::precheck $label FAILED"; rc=1
  fi
  echo "::endgroup::"
}

run "linux-arm64" $DOCKER codeterm-build-linux-arm64 bash /build-linux.sh "$TAG"
run "linux-x86_64" $DOCKER -e CARGO_BUILD_TARGET=x86_64-unknown-linux-gnu codeterm-build-linux-x64 bash /build-linux.sh "$TAG"
run "macos" bash -c "cd '$REPO' && mkdir -p dist && touch dist/index.html && cargo check --manifest-path src-tauri/Cargo.toml && cargo check --manifest-path src-tauri/Cargo.toml --target x86_64-apple-darwin"

if [ "$rc" -eq 0 ]; then
  echo "PRECHECK_OK $TAG — all platforms compile, safe to build"
else
  echo "::error::precheck failed — fix the compile error before the full build"
fi
exit "$rc"
