#!/usr/bin/env bash
set -euo pipefail
REPO="$(git rev-parse --show-toplevel)"
node "${REPO}/scripts/check-mux-write.mjs" "${REPO}/src-tauri"
