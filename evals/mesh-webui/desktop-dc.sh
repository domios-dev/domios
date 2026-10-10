#!/usr/bin/env bash
set -euo pipefail
[ "${B5_GO_LIVE:-}" = yes ] || exit 2
unset CODETERM_INSTANCE
export CODETERM_API_PORT=7686
source /c/Developer/codeterm/codeterm/evals/_lib/lib.sh
[ "$EVAL_LIB_DEV_API_PORT" = 7686 ] && [ -z "$EVAL_LIB_RESOLVED_INSTANCE" ] || exit 7
dc "$@"
