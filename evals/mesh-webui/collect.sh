#!/usr/bin/env bash
set -euo pipefail
: "${B2_GO_LIVE:?Requires Orchestrator GO live}"
[ "$B2_GO_LIVE" = yes ] || exit 2
peer="${1:?Exact Windows DEV peer id required}"
out="${2:?Absolute target-check/evals/mesh-webui-RUN path required}"
export CODETERM_INSTANCE=aiderqa CODETERM_API_PORT=7697
source /Users/dev/Developer/codeterm-worktrees/rc-mac/evals/_lib/lib.sh
[ "$EVAL_LIB_DEV_API_PORT" = 7697 ] || exit 7
[ "$EVAL_LIB_RESOLVED_INSTANCE" = aiderqa ] || exit 7
mkdir -p "$out"
dc tab list > "$out/mac-tabs.txt"
dc mesh status --json > "$out/mac-mesh.json"
dc tab list --mesh "$peer" > "$out/windows-tabs.txt"
dc mesh status --mesh "$peer" --json > "$out/windows-mesh.json"
