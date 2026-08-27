#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-/Users/user/Developer/.psetup-target}"

cargo test --manifest-path "$ROOT/src-tauri/Cargo.toml" --test snapshot_testbench -- --nocapture
