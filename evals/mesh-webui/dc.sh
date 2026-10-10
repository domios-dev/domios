#!/usr/bin/env bash
set -euo pipefail
[ "${B2_GO_LIVE:-}" = yes ] || { echo 'Requires GO live' >&2; exit 2; }
export CODETERM_INSTANCE=aiderqa CODETERM_API_PORT=7697
source /Users/kandrey/Developer/codeterm-worktrees/rc-mac/evals/_lib/lib.sh
[ "$EVAL_LIB_DEV_API_PORT" = 7697 ] && [ "$EVAL_LIB_RESOLVED_INSTANCE" = aiderqa ] || exit 7
dc "$@"
