#!/usr/bin/env bash
set -euo pipefail
[ "${B2_GO_LIVE:-}" = yes ] || { echo 'Requires Orchestrator GO live' >&2; exit 2; }
peer="${1:?Exact Windows DEV peer ID}"
manifest="${2:?Populated manifest path}"
run="${3:?Unique run name}"
[[ "$run" =~ ^[a-zA-Z0-9_-]+$ ]] || exit 2
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
rc="$(cd "$here/../.." && pwd)"
pad="$here"
cd "$rc"
node "$pad/preflight.mjs" "$manifest" "$peer"
status=0
for direction in mac-web-to-windows windows-web-to-mac; do
  for provider in claude codex opencode grok; do
    out="$rc/target-check/evals/mesh-webui-$run-$direction-$provider"
    if scripts/test-capped.sh --exec env B2_GO_LIVE=yes B2_DIRECTION="$direction" B2_PROVIDER="$provider" node "$pad/run.mjs" "$manifest" "$out"; then
      :
    else
      status=1
    fi
  done
done
node "$pad/combine.mjs" "$run"
exit "$status"
