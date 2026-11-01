#!/usr/bin/env bash
set -euo pipefail
[ "${DOMIOS_LIVE_PER_FRIEND:-}" = 1 ] && [ "$#" -eq 2 ] || exit 7
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
export CODETERM_INSTANCE=curated-second CODETERM_API_PORT=7717
unset CODETERM_CLI_PATH
source "$ROOT/evals/_lib/lib.sh"
dev_assert_target
dev_run node "$ROOT/evals/neighbor-friend-receiving/restart.mjs" "$1" "$2"
