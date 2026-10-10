#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
if [ "${1:-}" = --help ]; then
  echo 'Usage: bash evals/restart-endurance/run.sh --out DIR --notice-hook FILE [--continue-from-setup] [--model PROVIDER=MODEL]'
  echo 'Offline: bash evals/restart-endurance/run.sh --dry-run --out DIR --from-results FILE --log-dir DIR'
  echo 'Authorized QA only. Real LLM turns; three selected DEV restarts. See README.md.'
  exit 0
fi
if [ "${1:-}" = --dry-run ]; then
  shift
  node "$ROOT/evals/restart-endurance/dry-run.mjs" "$@"
  exit
fi
unset CODETERM_CLI_PATH
source "$ROOT/evals/_lib/lib.sh"
ENDURE_BASH="${BASH:-bash}"
if is_windows; then ENDURE_BASH="$(cygpath -w "$ENDURE_BASH")"; fi
export ENDURE_BASH
if [ "${1:-}" = --dc ]; then
  shift
  # A lazy tab has no session yet; the lib's optional grep may return 1.
  if dc "$@"; then exit 0; else exit "$?"; fi
fi
OUT= NOTICE_HOOK= CONTINUE_SETUP=0
MODELS=()
while [ "$#" -gt 0 ]; do
  case "$1" in
    --out) OUT="${2:?missing output directory}"; shift 2 ;;
    --continue-from-setup) CONTINUE_SETUP=1; shift ;;
    --notice-hook) NOTICE_HOOK="${2:?missing notice hook}"; shift 2 ;;
    --model) MODELS+=("${2:?missing provider=model override}"); shift 2 ;;
    *) echo "Unknown argument: $1" >&2; exit 2 ;;
  esac
done
ENDURE_MODEL_OVERRIDES="$(node -e 'const models = {}; for (const value of process.argv.slice(1)) { const split = value.indexOf("="); if (split < 1 || split === value.length - 1) throw new Error("Expected provider=model"); models[value.slice(0, split)] = value.slice(split + 1); } process.stdout.write(JSON.stringify(models));' ${MODELS[@]+"${MODELS[@]}"})"
export ENDURE_MODEL_OVERRIDES
[ -n "$OUT" ] && [ -f "$NOTICE_HOOK" ] || {
  echo 'Both --out DIR and --notice-hook FILE are required; see --help.' >&2
  exit 2
}
mkdir -p "$OUT"
OUT="$(cd "$OUT" && pwd)"
NOTICE_HOOK="$(cd "$(dirname "$NOTICE_HOOK")" && pwd)/$(basename "$NOTICE_HOOK")"
dev_assert_restart_target 1
if [ -n "$EVAL_LIB_RESOLVED_INSTANCE" ]; then
  [ "$EVAL_LIB_RESOLVED_INSTANCE" = aiderqa ] && [ "$EVAL_LIB_DEV_API_PORT" = 7697 ] || {
    echo 'This named-instance trial is restricted to aiderqa on 7697.' >&2
    exit 2
  }
fi
cd "$ROOT"
if [ -f "$OUT/complete" ]; then
  cat "$OUT/results.md"
  exit "$(cat "$OUT/complete")"
fi
[ ! -e "$OUT/results.json" ] || [ "$CONTINUE_SETUP" -eq 1 ] || {
  echo 'Incomplete run preserved. Refusing automatic message replay; use a new output directory after reviewing its manifest.' >&2
  exit 2
}
mkdir "$OUT/run.lock" || { echo 'Output directory already has an active run lock.' >&2; exit 2; }
trap 'rmdir "$OUT/run.lock" 2>/dev/null || true' EXIT
if [ "$CONTINUE_SETUP" -eq 1 ]; then
  node - "$OUT/results.json" <<'JS'
const fs = require('node:fs');
const result = JSON.parse(fs.readFileSync(process.argv[2], 'utf8'));
if (!result.setup_complete || result.rounds.some((round) => round.round > 0) || result.fatal) {
  throw new Error('Only an explicitly completed setup with no restart round can continue.');
}
JS
else
  dev_run node "$ROOT/evals/restart-endurance/observe.mjs" setup "$OUT"
fi
for ROUND in 1 2 3; do
  echo "NOTICE: Endure DEV $EVAL_LIB_DEV_API_PORT restart $ROUND/3 begins now."
  node "$ROOT/evals/restart-endurance/notice.mjs" "$NOTICE_HOOK" "$ROUND" > "$OUT/notice-$ROUND.log" 2>&1
  dev_run node "$ROOT/evals/restart-endurance/observe.mjs" before "$OUT" "$ROUND"
  restarted=0
  if dev_run bash "$ROOT/build.sh" dev-prebuilt --wait --wait-build > "$OUT/restart-$ROUND.log" 2>&1; then
    restarted=1
  elif grep -q "refusing before stopping dev" "$OUT/restart-$ROUND.log"; then
    # Frontend commits made after the DEV build invalidate prebuilt provenance; an incremental dev build restarts honestly.
    echo "Prebuilt provenance is stale; restarting with an incremental dev build." >> "$OUT/restart-$ROUND.log"
    dev_run bash "$ROOT/build.sh" dev --wait --wait-build >> "$OUT/restart-$ROUND.log" 2>&1 && restarted=1
  fi
  if [ "$restarted" -ne 1 ]; then
    if dev_run node "$ROOT/evals/restart-endurance/observe.mjs" health "$OUT" "$ROUND" >> "$OUT/restart-$ROUND.log" 2>&1; then
      restarted=1
    fi
  fi
  if [ "$restarted" -ne 1 ]; then
    dev_run node "$ROOT/evals/restart-endurance/observe.mjs" abort "$OUT" "$ROUND" || true
    echo "Restart failed; see $OUT/restart-$ROUND.log." >&2
    exit 2
  fi
  dev_run node "$ROOT/evals/restart-endurance/observe.mjs" round "$OUT" "$ROUND"
done
dev_run node "$ROOT/evals/restart-endurance/observe.mjs" finish "$OUT"
exit "$(cat "$OUT/complete")"
