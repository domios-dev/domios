#!/usr/bin/env bash
# Caller: build.sh script-guards/release-preflight; CI file-sizes
set -euo pipefail
export CODETERM_LAYERING_KNOWN_TAURI=''
node "$(dirname "${BASH_SOURCE[0]}")/check-crate-layering.mjs" "$@"
