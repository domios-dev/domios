#!/usr/bin/env bash
# Windows lane: cross-compile check against x86_64-pc-windows-msvc.
# Catches #[cfg(windows)] compile breakage locally in minutes without a
# Windows machine. Execution of the compiled tests belongs to a Windows
# mesh peer (see docs/superpowers/plans/2026-06-12-cross-platform-test-matrix.md).
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel)"
IMAGE=codeterm-test-windows-cross

docker build -q -t "$IMAGE" -f "$ROOT/docker/test-windows-cross.dockerfile" "$ROOT/docker" >/dev/null

docker run --rm \
  -v "$ROOT:/work" \
  -v codeterm-xwin-cargo-registry:/root/.cargo/registry \
  -v codeterm-xwin-cache:/root/.cache/cargo-xwin \
  -v codeterm-xwin-target:/work/src-tauri/target \
  -v codeterm-xwin-cli-target:/work/cli/target \
  -e XWIN_ACCEPT_LICENSE=1 \
  -w /work \
  "$IMAGE" bash -c '
    set -euo pipefail
    mkdir -p dist
    # tauri_build validates externalBin against the TARGET triple, and
    # prepare-embed-stub.js only knows the host triple — touch it directly.
    mkdir -p src-tauri/bundled-embed
    touch src-tauri/bundled-embed/codeterm-embed-x86_64-pc-windows-msvc.exe
    mkdir -p src-tauri/bundled-cli
    [ -e src-tauri/bundled-cli/codeterm ] || touch src-tauri/bundled-cli/codeterm
    echo "=== cargo check x86_64-pc-windows-msvc: src-tauri ==="
    cargo xwin check --locked -j 4 --target x86_64-pc-windows-msvc \
      --manifest-path src-tauri/Cargo.toml --lib --tests
    echo "=== cargo check x86_64-pc-windows-msvc: cli ==="
    cargo xwin check --locked -j 4 --target x86_64-pc-windows-msvc \
      --manifest-path cli/Cargo.toml --tests
  '
