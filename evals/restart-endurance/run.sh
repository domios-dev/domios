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
node "$ROOT/evals/restart-endurance/target.mjs" "${CODETERM_INSTANCE:-}" "${CODETERM_API_PORT:-}"
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
node "$ROOT/evals/restart-endurance/target.mjs" "$EVAL_LIB_RESOLVED_INSTANCE" "$EVAL_LIB_DEV_API_PORT"
dev_assert_restart_target 0
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
  node --input-type=module - "$OUT" <<'JS'
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { resumeSetup } from "./evals/restart-endurance/resume.mjs";
const out = process.argv[2];
const path = join(out, "results.json");
const original = readFileSync(path, "utf8");
const now = new Date().toISOString();
const result = resumeSetup(JSON.parse(original), [1, 2, 3].some((n) => existsSync(join(out, `restart-${n}.log`))), now);
writeFileSync(join(out, `results-before-resume-${now.replaceAll(":", "-")}.json`), original);
writeFileSync(path, `${JSON.stringify(result, null, 2)}\n`);
JS
else
  dev_run node "$ROOT/evals/restart-endurance/observe.mjs" setup "$OUT"
fi
for ROUND in 1 2 3; do
  echo "NOTICE: Endure DEV $EVAL_LIB_DEV_API_PORT restart $ROUND/3 begins now."
  node "$ROOT/evals/restart-endurance/notice.mjs" "$NOTICE_HOOK" "$ROUND" > "$OUT/notice-$ROUND.log" 2>&1
  dev_run node "$ROOT/evals/restart-endurance/observe.mjs" before "$OUT" "$ROUND"
  if ! dev_run node "$ROOT/evals/restart-endurance/launch.mjs" "$OUT" "$ROUND" > "$OUT/restart-$ROUND.log" 2>&1; then
    dev_run node "$ROOT/evals/restart-endurance/observe.mjs" abort "$OUT" "$ROUND" || true
    echo "restart_not_observed; see $OUT/restart-$ROUND.log and restart-$ROUND.json." >&2
    exit 1
  fi
  dev_run node "$ROOT/evals/restart-endurance/observe.mjs" round "$OUT" "$ROUND"
done
dev_run node "$ROOT/evals/restart-endurance/observe.mjs" finish "$OUT"
exit "$(cat "$OUT/complete")"
