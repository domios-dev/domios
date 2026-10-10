#!/usr/bin/env bash
set -euo pipefail
repo_root="$(git rev-parse --show-toplevel)"
exec node "$repo_root/evals/mesh-two-dev/capture.mjs" "${1:?Exact meshqa peer id required after GO live}" "${2:?Run directory required}"
