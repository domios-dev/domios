#!/usr/bin/env bash
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
HERE="$ROOT/evals/core-journeys"

usage() {
  cat <<'EOF'
Usage: bash evals/core-journeys/run.sh MODE [--out DIR] [driver flags]
  all       setup -> bench-owned DEV restart (build.sh kill + dev-prebuilt) -> verify -> cleanup
  setup     J1-J3 and the pre-restart snapshot; leaves fixtures for verify
  verify    J4 (+ J3 after restart) against the state in --out; run after an operator restart
  cleanup   remove the fixtures recorded in --out
Driver flags: --model PROVIDER=ADVERTISED_ID (repeat for every installed provider)  --providers a,b  --history-kb N  --deadline-secs N  --all-tabs  --keep
--plan prints the journeys, steps and evidence paths without contacting an instance.
Needs a named instance and its own API port; ports 7685 and 7686 are refused.
Check a finished run: node evals/core-journeys/lib/evidence-contract.mjs OUT
Spends real provider tokens. See evals/core-journeys/README.md.
EOF
}

if [ "${1:-}" = --dc ]; then
  shift
  source "$ROOT/evals/_lib/lib.sh"
  source "$HERE/lib/instance-identity.sh"
  if dc "$@"; then exit 0; else exit "$?"; fi
fi
case "${1:-}" in
  all|setup|verify|cleanup) MODE="$1"; shift ;;
  -h|--help|"") usage; exit 0 ;;
  *) echo "Unknown mode: $1" >&2; usage >&2; exit 2 ;;
esac

OUT="" PLAN=0 PASS=()
while [ "$#" -gt 0 ]; do
  case "$1" in
    --plan) PLAN=1; shift ;;
    --out) OUT="${2:?--out needs a directory}"; shift 2 ;;
    *) PASS+=("$1"); shift ;;
  esac
done

source "$ROOT/evals/_lib/lib.sh"
source "$HERE/lib/instance-identity.sh"
REQUESTED_INSTANCE="$(printf '%s' "$EVAL_LIB_REQUESTED_INSTANCE" | tr -d '[:space:]')"
export CORE_JOURNEY_REQUESTED_INSTANCE="$REQUESTED_INSTANCE"
export CORE_JOURNEY_REQUESTED_API_PORT="$EVAL_LIB_REQUESTED_API_PORT"
select_build_identity() {
  core_journey_select_build_identity "$1"
}
select_build_identity "$ROOT" || { echo 'FAIL could not read the build-under-test CLI identity and environment contract' >&2; exit 7; }
[ -n "$REQUESTED_INSTANCE" ] || { echo "FAIL named instance is unset: core journeys run only on a named isolated instance" >&2; exit 7; }
case "$EVAL_LIB_REQUESTED_API_PORT" in
  7685) echo "FAIL refusing production port 7685" >&2; exit 7 ;;
  7686) echo "FAIL refusing the shared DEV port 7686; use the named instance's own port" >&2; exit 7 ;;
esac

OUT="${OUT:-$ROOT/target-check/evals/core-journeys/$(date +%Y%m%d-%H%M%S)-$$}"
if [ "$PLAN" = 1 ]; then
  command -v node >/dev/null || { echo "FAIL missing prerequisite: node" >&2; exit 7; }
  exec node "$HERE/lib/plan.mjs" "$MODE" --out "$OUT" --instance "$REQUESTED_INSTANCE" \
    ${EVAL_LIB_REQUESTED_API_PORT:+--port "$EVAL_LIB_REQUESTED_API_PORT"}
fi

for prerequisite in node python3 git; do
  command -v "$prerequisite" >/dev/null || { echo "FAIL missing prerequisite: $prerequisite" >&2; exit 7; }
done
dev_resolve || { echo "FAIL dev connection unresolved" >&2; exit 7; }
case "$EVAL_LIB_DEV_API_PORT" in
  7685|7686) echo "FAIL instance resolved to shared port $EVAL_LIB_DEV_API_PORT" >&2; exit 7 ;;
esac
[ -n "$EVAL_LIB_RESOLVED_INSTANCE" ] || { echo "FAIL resolved target has no instance name" >&2; exit 7; }

if [ "$MODE" != cleanup ]; then
  dev_run node "$HERE/prerequisites.mjs" || exit 7
fi

mkdir -p "$OUT" && OUT="$(cd "$OUT" && pwd)"
STARTED=$SECONDS

driver() { dev_run node "$HERE/driver.mjs" "$1" --out "$2" ${PASS[@]+"${PASS[@]}"} "${@:3}"; }
restart() { dev_run node "$HERE/restart.mjs" --out "$1" "${@:2}"; }

# A model-policy refusal ends paid paths, including restart; other journey failures retain the full cycle.
cycle() {
  local out="$1" status=0
  shift
  local setup_status=0
  driver setup "$out" || setup_status=$?
  if [ "$setup_status" != 0 ] && [ "$setup_status" != 1 ]; then
    driver cleanup "$out" || return "$setup_status"
    return "$setup_status"
  fi
  [ "$setup_status" = 0 ] || status=1
  dev_assert_restart_target 1 || return 7
  restart "$out" "$@" || status=1
  driver verify "$out" ${VERIFY_EXTRA[@]+"${VERIFY_EXTRA[@]}"} || status=1
  driver cleanup "$out" || status=1
  return "$status"
}

case "$MODE" in
  setup|verify|cleanup) driver "$MODE" "$OUT"; STATUS=$? ;;
  all) cycle "$OUT"; STATUS=$? ;;

esac

echo "CORE JOURNEYS $MODE exit=$STATUS wall=$((SECONDS - STARTED))s out=$OUT"
exit "$STATUS"
