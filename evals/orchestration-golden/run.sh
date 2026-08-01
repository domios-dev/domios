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
REPORT_LOG="$OUT_DIR/orchestration-golden.log"
exec 3>>"$REPORT_LOG"

declare -a ROWS=()

report() {
  printf '%s\n' "$*"
  printf '%s\n' "$*" >&3
}

reportf() {
  local format="$1"
  shift
  printf "$format" "$@"
  printf "$format" "$@" >&3
}

check_dev_precondition() {
  if curl -fsS http://127.0.0.1:7686/api/health >/dev/null 2>&1; then
    return 0
  fi

  report "ABORT: dev daemon not healthy on 7686. Start it (orchestrator owns dev): ./build.sh dev"
  report "report: $REPORT_LOG"
  return 7
}

run_harness() {
  local name="$1"
  local command="$2"
  local log="$OUT_DIR/${name}.log"
  local start end elapsed rc status

  report
  report "== $name =="
  report "command: $command"
  report "log: $log"

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
  report "$status: $name rc=$rc elapsed=${elapsed}s"
  if [ "$rc" -ne 0 ]; then
    report "last log lines:"
    while IFS= read -r line; do
      report "$line"
    done < <(tail -40 "$log")
  fi
}

print_summary() {
  local row name status rc elapsed command log pass fail
  pass=0
  fail=0

  report
  report "======================================================================"
  report " ORCHESTRATION GOLDEN — summary"
  report "======================================================================"
  reportf " %-30s %-7s %-4s %-8s %s\n" "HARNESS" "STATUS" "RC" "ELAPSED" "LOG"
  report "----------------------------------------------------------------------"
  for row in "${ROWS[@]}"; do
    IFS='|' read -r name status rc elapsed command log <<< "$row"
    reportf " %-30s %-7s %-4s %-8s %s\n" "$name" "$status" "$rc" "$elapsed" "$log"
    case "$status" in
      PASS) pass=$((pass + 1)) ;;
      *) fail=$((fail + 1)) ;;
    esac
  done
  report "----------------------------------------------------------------------"
  report " PASS=$pass  FAIL=$fail"
  report " logs=$OUT_DIR"
  report " report=$REPORT_LOG"
  report "======================================================================"
}

check_dev_precondition || exit $?

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
