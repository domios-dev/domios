#!/usr/bin/env bash
# Caller: build.sh script-guards; scripts/tests/crate-layering.test.mjs
set -euo pipefail
export CODETERM_LAYERING_KNOWN_TAURI='codeterm-events:task_kfzm58 codeterm-keepawake:task_kfzm58'
node "$(dirname "${BASH_SOURCE[0]}")/check-crate-layering.mjs" "$@"
