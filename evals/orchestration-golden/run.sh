#!/usr/bin/env bash
# One-command orchestration golden tier.
#
# Consolidation only: this script sequences the existing orchestration e2e
# harnesses and reports their verdicts. It does not add or alter test semantics.
#
# Usage:
#   bash evals/orchestration-golden/run.sh
#
# Optional:
#   ORCH_GOLDEN_OUT_DIR=/path/to/logs bash evals/orchestration-golden/run.sh

set -u
set -o pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
OUT_DIR="${ORCH_GOLDEN_OUT_DIR:-$REPO_ROOT/target-check/evals/orchestration-golden/$(date +%Y%m%d-%H%M%S)}"
mkdir -p "$OUT_DIR"

declare -a ROWS=()

run_harness() {
  local name="$1"
  local command="$2"
  local log="$OUT_DIR/${name}.log"
  local start end elapsed rc status

  echo
  echo "== $name =="
  echo "command: $command"
  echo "log: $log"

  start="$(date +%s)"
  (
    cd "$REPO_ROOT" || exit 1
    eval "$command"
  ) >"$log" 2>&1
  rc=$?
  end="$(date +%s)"
  elapsed=$((end - start))

  if [ "$rc" -eq 0 ]; then
    status="PASS"
  else
    status="FAIL"
  fi

  ROWS+=("$name|$status|$rc|${elapsed}s|$command|$log")
  echo "$status: $name rc=$rc elapsed=${elapsed}s"
  if [ "$rc" -ne 0 ]; then
    echo "last log lines:"
    tail -40 "$log" || true
  fi
}

print_summary() {
  local row name status rc elapsed command log pass fail
  pass=0
  fail=0

  echo
  echo "======================================================================"
  echo " ORCHESTRATION GOLDEN — summary"
  echo "======================================================================"
  printf " %-30s %-7s %-4s %-8s %s\n" "HARNESS" "STATUS" "RC" "ELAPSED" "LOG"
  echo "----------------------------------------------------------------------"
  for row in "${ROWS[@]}"; do
    IFS='|' read -r name status rc elapsed command log <<< "$row"
    printf " %-30s %-7s %-4s %-8s %s\n" "$name" "$status" "$rc" "$elapsed" "$log"
    case "$status" in
      PASS) pass=$((pass + 1)) ;;
      *) fail=$((fail + 1)) ;;
    esac
  done
  echo "----------------------------------------------------------------------"
  echo " PASS=$pass  FAIL=$fail"
  echo " logs=$OUT_DIR"
  echo "======================================================================"
}

run_harness "orchestrator-creation-matrix" "bash evals/orchestrator-creation-matrix/run.sh"
run_harness "restore-matrix" "bash evals/restore-matrix/run.sh"
run_harness "orchestrator-refresh-drill" "bash evals/orchestrator-refresh/drill.sh"
run_harness "report-round-trip" "bash evals/plan-lifecycle/run.sh"

print_summary

for row in "${ROWS[@]}"; do
  IFS='|' read -r _ status _ _ _ _ <<< "$row"
  [ "$status" = "PASS" ] || exit 1
done

exit 0
