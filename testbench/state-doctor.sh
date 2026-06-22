#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-/Users/kandrey/Developer/.psetup-target}"

cd "$ROOT/src-tauri"
cargo test --test state_doctor state_dump_quality_gate -- --ignored --nocapture
