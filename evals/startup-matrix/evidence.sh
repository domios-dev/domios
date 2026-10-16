#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
if [ "${1:-}" = --help ]; then
  echo 'Usage: bash evals/startup-matrix/evidence.sh browser|recovery|phases [options]'
  echo 'See SCENARIOS.md. Browser/recovery require --dry-run or explicit active mode.'
  exit 0
fi
scenario="${1:?scenario required}"; shift
case "$scenario" in
  phases) exec node "$ROOT/evals/startup-matrix/phase-evidence.mjs" "$@" ;;
  browser|recovery) ;;
  *) echo 'Unknown evidence scenario' >&2; exit 2 ;;
esac
unset CODETERM_INSTANCE CODETERM_API_PORT CODETERM_CLI_PATH
source "$ROOT/evals/_lib/lib.sh"
dev_run node "$ROOT/evals/startup-matrix/${scenario}-evidence.mjs" "$@"
