#!/usr/bin/env bash
# Internal QA gate runner — serial named gates on a warm checkout.
#
# Repo-local tooling for orchestrators/build machines. No daemon/socket required.
# Machine-parseable first line (plan-stride friendly); prose log follows.
#
# Usage:
#   scripts/qa-gate.sh list
#   scripts/qa-gate.sh run GATE [GATE...] [options]
#
# Options:
#   --ref REF           checkout REF before running (fetch origin first with --fetch)
#   --expect-sha SHA    require HEAD to match SHA prefix after checkout
#   --fetch             git fetch origin before --ref checkout
#   --timeout SECS      per-gate timeout (default from gate registry)
#   --allow-dirty       skip clean working-tree check
#   --json              JSON lines instead of KV first line
#   --skip-dev-check    run dev-dependent gates even if 7686 is down
#
# Prod orchestrator tabs leak CODETERM_API_PORT=7685 — gates auto-sanitize to dev (7686).
# QA_GATE_ENV_LOCKED=1 preserves env; QA_GATE_INSTANCE sets named dev instance.
#
# Examples:
#   scripts/qa-gate.sh run fmt check --ref feat/canvas-updates --expect-sha a2523df6
#   scripts/qa-gate.sh run prepush-full --timeout 2400
#   scripts/qa-gate.sh run macos-native --allow-dirty
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
# shellcheck source=scripts/qa-gates.lib.sh
source "$ROOT/scripts/qa-gates.lib.sh"
QA_ROOT="$ROOT"
qa_sanitize_env

qa_cmd_list() {
  echo "Available gates (run serially with: scripts/qa-gate.sh run GATE ...):"
  while IFS= read -r g; do
    [ -n "$g" ] || continue
    printf '  %-20s %ss  %s\n' "$g" "$(qa_gate_timeout "$g")" "$(qa_gate_desc "$g")"
  done < <(qa_list_gates)
}

usage() {
  sed -n '2,22p' "$0" | sed 's/^# \{0,1\}//'
  exit 2
}

GATES=()
REF=""
EXPECT_SHA=""
DO_FETCH=0
ALLOW_DIRTY=0
JSON_OUT=0
SKIP_DEV_CHECK=0
GLOBAL_TIMEOUT=""

while [ $# -gt 0 ]; do
  case "$1" in
    list) qa_cmd_list; exit 0 ;;
    run) shift; break ;;
    -h|--help) usage ;;
    *) GATES+=("$1"); shift ;;
  esac
done

while [ $# -gt 0 ]; do
  case "$1" in
    --ref) REF="${2:?}"; shift 2 ;;
    --expect-sha) EXPECT_SHA="${2:?}"; shift 2 ;;
    --fetch) DO_FETCH=1; shift ;;
    --timeout) GLOBAL_TIMEOUT="${2:?}"; shift 2 ;;
    --allow-dirty) ALLOW_DIRTY=1; shift ;;
    --json) JSON_OUT=1; shift ;;
    --skip-dev-check) SKIP_DEV_CHECK=1; shift ;;
    -h|--help) usage ;;
    *) GATES+=("$1"); shift ;;
  esac
done

if [ ${#GATES[@]} -eq 0 ]; then
  echo "error: no gates specified (try: scripts/qa-gate.sh list)" >&2
  usage
fi

qa_verify_clean() {
  if [ "$ALLOW_DIRTY" = 1 ]; then return 0; fi
  local dirty
  dirty="$(git -C "$ROOT" status --porcelain 2>/dev/null || true)"
  if [ -n "$dirty" ]; then
    qa_report_run blocked 0 0 "dirty working tree (use --allow-dirty to override)"
    exit 2
  fi
}

qa_checkout_ref() {
  if [ -z "$REF" ]; then return 0; fi
  if [ "$DO_FETCH" = 1 ]; then
    git -C "$ROOT" fetch origin "$REF" 2>&1 || git -C "$ROOT" fetch origin 2>&1 || true
  fi
  git -C "$ROOT" checkout "$REF" 2>&1
  if [ -n "$EXPECT_SHA" ]; then
    local head
    head="$(git -C "$ROOT" rev-parse HEAD)"
    case "$head" in
      "$EXPECT_SHA"*) ;;
      *)
        qa_report_run blocked 0 0 "HEAD $head does not match --expect-sha $EXPECT_SHA"
        exit 2
        ;;
    esac
  fi
}

qa_report_gate() {
  local gate="$1" status="$2" duration="$3" detail="${4:-}"
  local sha branch host
  sha="$(qa_sha)"
  branch="$(qa_branch)"
  host="$(qa_host)"
  if [ "$JSON_OUT" = 1 ]; then
    python3 -c '
import json,sys
print(json.dumps({
  "kind":"qa_gate","gate":sys.argv[1],"status":sys.argv[2],
  "duration_secs":int(sys.argv[3]),"sha":sys.argv[4],"branch":sys.argv[5],
  "host":sys.argv[6],"detail":sys.argv[7] or None
}))' "$gate" "$status" "$duration" "$sha" "$branch" "$host" "$detail"
  else
    qa_emit QA_GATE \
      "gate=$gate" "status=$status" "duration_secs=$duration" \
      "sha=$sha" "branch=$branch" "host=$host"
    if [ -n "$detail" ]; then
      echo "detail=$detail"
    fi
  fi
  return 0
}

qa_report_run() {
  local status="$1" passed="$2" failed="$3" detail="${4:-}"
  local sha branch host duration="${QA_RUN_DURATION:-0}"
  sha="$(qa_sha)"
  branch="$(qa_branch)"
  host="$(qa_host)"
  if [ "$JSON_OUT" = 1 ]; then
    python3 -c '
import json,sys
print(json.dumps({
  "kind":"qa_run","status":sys.argv[1],"passed":int(sys.argv[2]),
  "failed":int(sys.argv[3]),"duration_secs":int(sys.argv[4]),
  "sha":sys.argv[5],"branch":sys.argv[6],"host":sys.argv[7],
  "detail":sys.argv[8] or None
}))' "$status" "$passed" "$failed" "$duration" "$sha" "$branch" "$host" "$detail"
  else
    qa_emit QA_RUN \
      "status=$status" "passed=$passed" "failed=$failed" \
      "duration_secs=$duration" "sha=$sha" "branch=$branch" "host=$host"
    if [ -n "$detail" ]; then
      echo "detail=$detail"
    fi
  fi
  return 0
}

qa_verify_clean
qa_checkout_ref

RUN_START=$SECONDS
PASSED=0
FAILED=0
OVERALL=pass
LOG_DIR="${TMPDIR:-/tmp}/codeterm-qa-gate-$$"
mkdir -p "$LOG_DIR"

for gate in "${GATES[@]}"; do
  if ! qa_list_gates | grep -qx "$gate"; then
    qa_report_gate "$gate" blocked 0 "unknown gate"
    FAILED=$((FAILED + 1))
    OVERALL=blocked
    continue
  fi
  if ! qa_gate_platform "$gate"; then
    qa_report_gate "$gate" skip 0 "platform unavailable on $(uname -s)"
    continue
  fi
  if qa_gate_needs_dev "$gate" && [ "$SKIP_DEV_CHECK" != 1 ] && ! qa_dev_healthy; then
    qa_report_gate "$gate" skip 0 "dev :${CODETERM_API_PORT:-7686} not healthy"
    continue
  fi

  timeout_secs="${GLOBAL_TIMEOUT:-$(qa_gate_timeout "$gate")}"
  gate_log="$LOG_DIR/$gate.log"
  gate_start=$SECONDS
  echo "=== running gate: $gate (timeout ${timeout_secs}s) ===" >&2

  set +e
  qa_run_timeout "$timeout_secs" bash -c "source '$ROOT/scripts/qa-gates.lib.sh'; QA_ROOT='$ROOT'; qa_run_gate_body '$gate'" \
    >"$gate_log" 2>&1
  rc=$?
  gate_dur=$((SECONDS - gate_start))

  if [ "$rc" -eq 0 ]; then
    qa_report_gate "$gate" pass "$gate_dur"
    PASSED=$((PASSED + 1))
  elif [ "$rc" -eq 124 ]; then
    qa_report_gate "$gate" timeout "$gate_dur" "exceeded ${timeout_secs}s cap"
    FAILED=$((FAILED + 1))
    OVERALL=fail
  else
    tail_line="$(tail -n 3 "$gate_log" 2>/dev/null | tr '\n' ' ' | sed 's/  */ /g')"
    qa_report_gate "$gate" fail "$gate_dur" "${tail_line:-exit $rc}"
    FAILED=$((FAILED + 1))
    OVERALL=fail
  fi
  if [ "$JSON_OUT" != 1 ]; then
    echo "--- log: $gate_log ---"
    tail -n 40 "$gate_log" 2>/dev/null || true
    echo
  fi
done

QA_RUN_DURATION=$((SECONDS - RUN_START))
qa_report_run "$OVERALL" "$PASSED" "$FAILED"
echo "logs: $LOG_DIR" >&2

if [ "$OVERALL" = pass ] && [ "$FAILED" -eq 0 ]; then exit 0; fi
if [ "$OVERALL" = blocked ]; then exit 2; fi
exit 1
