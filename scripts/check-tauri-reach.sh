#!/usr/bin/env bash
# Caller: build.sh verify/release-preflight; CI file-sizes; scripts/qa-gates.lib.sh
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
node scripts/check-tauri-reach.mjs "$@"
