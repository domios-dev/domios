#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
[ "${CODETERM_INSTANCE:-}" = restartqa ] && [ "${CODETERM_API_PORT:-}" = 7692 ] || {
  echo 'FAIL preflight: requires CODETERM_INSTANCE=restartqa CODETERM_API_PORT=7692'; exit 7;
}
: "${RL_EXPECTED_SHA:?Orchestrator must supply the merged release SHA}"
: "${RL_PREBUILT_CWD:?Orchestrator must supply the canonical prebuilt checkout}"
: "${RL_PAD_DIR:?Set to the calling agent pad directory before selecting DEV}"
source "$REPO_ROOT/evals/_lib/lib.sh"
dev_get /api/health > "$RL_PAD_DIR/restartqa-health.json"
python3 - "$RL_PAD_DIR/restartqa-health.json" "$RL_EXPECTED_SHA" <<'PY'
import json,sys
health=json.load(open(sys.argv[1])); actual=health.get('build','')
if not actual or not sys.argv[2].startswith(actual):
    raise SystemExit(f'FAIL preflight: health build {actual!r}, expected {sys.argv[2]}')
PY
export RL_BENCH_ROOT="$REPO_ROOT"
export RL_RUN_DIR="${RL_RUN_DIR:-$EVAL_LIB_OUT_BASE/codex-$(date +%Y%m%d-%H%M%S)-$$}"
mkdir -p "$RL_RUN_DIR"
for mode in gui normal; do
  dev_run python3 "$SCRIPT_DIR/codex_matrix.py" "$mode" || exit "$?"
done
