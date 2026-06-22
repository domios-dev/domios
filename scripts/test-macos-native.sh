#!/usr/bin/env bash
# macOS lane: native host run. macOS cannot be containerized; the host
# IS the macOS test environment. Mirrors ./build.sh test + cli + TS tests.
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel)"
cd "$ROOT"
mkdir -p dist
node scripts/prepare-embed-stub.js
# Resource-glob stub: tauri_build only needs bundled-cli/* to match a file;
# unit tests never execute the bundled CLI.
mkdir -p src-tauri/bundled-cli
[ -e src-tauri/bundled-cli/codeterm ] || touch src-tauri/bundled-cli/codeterm

echo "=== Rust unit tests: src-tauri (macos, $(uname -m)) ==="
cargo test --locked --manifest-path src-tauri/Cargo.toml --lib

echo "=== Rust unit tests: cli (macos) ==="
cargo test --locked --manifest-path cli/Cargo.toml

echo "=== TypeScript unit tests (macos) ==="
pnpm install --frozen-lockfile
pnpm -s test:unit

echo "=== Plugin tests (macos) ==="
pnpm -s test:plugins
