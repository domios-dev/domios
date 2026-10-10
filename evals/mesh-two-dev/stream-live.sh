#!/usr/bin/env bash
set -euo pipefail
export CODETERM_INSTANCE=aiderqa CODETERM_API_PORT=7697
repo_root="$(git rev-parse --show-toplevel)"
source "$repo_root/evals/_lib/lib.sh"
dev_assert_target
dev_run node "$repo_root/evals/mesh-two-dev/stream-live.mjs" "$@"
