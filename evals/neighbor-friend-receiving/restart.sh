#!/usr/bin/env bash
set -euo pipefail
[ "${DOMIOS_LIVE_PER_FRIEND:-}" = 1 ] && [ "${DOMIOS_PER_FRIEND_RESTART_C:-}" = 1 ] || exit 7
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
EXPECTED_SHA="${1:?candidate SHA required}"
[[ "$EXPECTED_SHA" =~ ^[a-f0-9]{40}$ ]] || exit 7
[ "$(git -C "$ROOT" rev-parse HEAD)" = "$EXPECTED_SHA" ] || { echo 'Candidate drift; no restart.' >&2; exit 7; }
export CODETERM_INSTANCE=curated-second CODETERM_API_PORT=7717
export DOMIOS_ACCOUNT_TARGET=qa CODETERM_DEV_WATCH=0
unset CODETERM_CLI_PATH
source "$ROOT/evals/_lib/lib.sh"
dev_assert_target
dev_assert_restart_target 0
cd "$ROOT"
dev_run bash build.sh kill
dev_run bash build.sh dev-prebuilt --wait
