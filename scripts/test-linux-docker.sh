#!/usr/bin/env bash
# Linux lane: Rust + TS unit tests inside a Docker container.
# The repo is bind-mounted; target dirs and node_modules are shadowed by
# named volumes so Linux artifacts never touch the host checkout.
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel)"
IMAGE=codeterm-test-linux

docker build -q -t "$IMAGE" -f "$ROOT/docker/test-linux.dockerfile" "$ROOT/docker" >/dev/null

GIT_COMMON="$(cd "$ROOT" && cd "$(git rev-parse --git-common-dir)" && pwd)"
# A linked worktree keeps its git dir outside ROOT; mount it at the same path so git resolves inside.
docker run --rm \
  -v "$ROOT:/work" \
  -v "$GIT_COMMON:$GIT_COMMON:ro" \
  -v codeterm-linux-cargo-registry:/root/.cargo/registry \
  -v codeterm-linux-target:/work/src-tauri/target \
  -v codeterm-linux-cli-target:/work/cli/target \
  -v codeterm-linux-node-modules:/work/node_modules \
  -e CI=true \
  -w /work \
  "$IMAGE" bash -c '
    set -euo pipefail
    # Host-owned mounts are otherwise refused by git, and the CLI build SHA falls back to unknown.
    git -C / config --global --add safe.directory "*"
    mkdir -p dist
    node scripts/prepare-embed-stub.js
    mkdir -p src-tauri/bundled-cli
    [ -e src-tauri/bundled-cli/codeterm ] || touch src-tauri/bundled-cli/codeterm
    echo "=== Rust unit tests: src-tauri (linux, $(uname -m)) ==="
    cargo test --locked --manifest-path src-tauri/Cargo.toml --lib
    echo "=== Rust unit tests: cli (linux) ==="
    cargo test --locked --manifest-path cli/Cargo.toml
    echo "=== TypeScript unit tests (linux) ==="
    # Store inside the node_modules volume: same filesystem (hardlinks work),
    # cached across runs, and pnpm cannot fall back to /work/.pnpm-store on the host.
    pnpm install --frozen-lockfile --store-dir node_modules/.pnpm-store
    pnpm -s test:unit
    echo "=== Plugin tests (linux) ==="
    pnpm -s test:plugins
  '
