#!/usr/bin/env bash
set -euo pipefail

if [ "${1:-}" = --help ] || [ "$#" -ne 4 ] || [ "${1:-}" != --manifest ] || [ "${3:-}" != --out ]; then
  echo 'Usage: DOMIOS_LIVE_PER_FRIEND=1 bash evals/neighbor-friend-receiving/run.sh --manifest PRIVATE.json --out NEW_DIRECTORY'
  echo 'Requires four already qualified named QA peers, two friendships and saved local concierge setups. See README.md.'
  [ "${1:-}" = --help ] && exit 0
  exit 2
fi
[ "${DOMIOS_LIVE_PER_FRIEND:-}" = 1 ] || { echo 'Refused: DOMIOS_LIVE_PER_FRIEND=1 is required.' >&2; exit 7; }
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
export CODETERM_INSTANCE=acceptance-r161 CODETERM_API_PORT=7707
unset CODETERM_CLI_PATH
source "$ROOT/evals/_lib/lib.sh"
dev_assert_target
dev_run node "$ROOT/evals/neighbor-friend-receiving/scenario.mjs" "$2" "$4"
