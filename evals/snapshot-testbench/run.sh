#!/usr/bin/env bash
# Curated eval: the snapshot library must stay green when detection or provider plugins change.
set -euo pipefail
ROOT="$(git rev-parse --show-toplevel)"
source "$ROOT/evals/_lib/lib.sh"
cd "$ROOT/src-tauri"
exec cargo test -p codeterm-devtools --test snapshot_testbench -- --include-ignored --nocapture
