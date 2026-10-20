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
  upgrade   J5: --from-checkout N-1_CHECKOUT on a named disposable instance
Driver flags: --providers a,b  --history-kb N  --deadline-secs N  --all-tabs  --keep
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
  all|setup|verify|cleanup|upgrade) MODE="$1"; shift ;;
  -h|--help|"") usage; exit 0 ;;
  *) echo "Unknown mode: $1" >&2; usage >&2; exit 2 ;;
esac

OUT="" FROM="" PLAN=0 PASS=()
while [ "$#" -gt 0 ]; do
  case "$1" in
    --plan) PLAN=1; shift ;;
    --out) OUT="${2:?--out needs a directory}"; shift 2 ;;
    --from-checkout) FROM="${2:?--from-checkout needs a path}"; shift 2 ;;
    *) PASS+=("$1"); shift ;;
  esac
done

source "$ROOT/evals/_lib/lib.sh"
source "$HERE/lib/instance-identity.sh"
REQUESTED_INSTANCE="$(printf '%s' "$EVAL_LIB_REQUESTED_INSTANCE" | tr -d '[:space:]')"
export CORE_JOURNEY_REQUESTED_INSTANCE="$REQUESTED_INSTANCE"
export CORE_JOURNEY_REQUESTED_API_PORT="$EVAL_LIB_REQUESTED_API_PORT"
build_identity_field() { core_journey_build_field "$1" "$2"; }
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
    ${EVAL_LIB_REQUESTED_API_PORT:+--port "$EVAL_LIB_REQUESTED_API_PORT"} ${FROM:+--from "$FROM"}
fi

for prerequisite in node python3 git; do
  command -v "$prerequisite" >/dev/null || { echo "FAIL missing prerequisite: $prerequisite" >&2; exit 7; }
done
if [ "$MODE" = upgrade ]; then
  [ -n "$REQUESTED_INSTANCE" ] && [ -n "$EVAL_LIB_REQUESTED_API_PORT" ] || { echo "FAIL upgrade needs the named instance's own API port" >&2; exit 2; }
  [ -n "$FROM" ] && [ -x "$FROM/build.sh" ] || { echo "FAIL --from-checkout must name the build N-1 checkout" >&2; exit 2; }
  FROM="$(cd "$FROM" && pwd)"
  PREVIOUS_CLI_NAME="$(build_identity_field "$FROM" binaryName)"
  PREVIOUS_INSTANCE_ENV="$(build_identity_field "$FROM" request.instance)"
  PREVIOUS_PORT_ENV="$(build_identity_field "$FROM" request.port)"
  CORE_JOURNEY_PREVIOUS_CLI_INSTANCE_ENV="$(build_identity_field "$FROM" cli.instance)"
  export CORE_JOURNEY_PREVIOUS_CLI_INSTANCE_ENV
  [[ "$PREVIOUS_INSTANCE_ENV" == *_INSTANCE ]] && [[ "$PREVIOUS_PORT_ENV" == *_API_PORT ]] || {
      echo 'FAIL --from-checkout does not expose its own launcher and CLI contract' >&2; exit 2;
    }
  select_build_identity "$FROM" || { echo 'FAIL could not read the v1.14.9 build identity' >&2; exit 7; }
  # The named disposable instance has no CLI yet; use N-1's own build contract and binary.
  (cd "$FROM" && env "$PREVIOUS_INSTANCE_ENV=$REQUESTED_INSTANCE" "$PREVIOUS_PORT_ENV=$EVAL_LIB_REQUESTED_API_PORT" \
    bash ./build.sh dev --wait --wait-build) || { echo "FAIL build N-1 did not start" >&2; exit 1; }
  PREVIOUS_BINARY_NAME="$PREVIOUS_CLI_NAME"
  [[ "$OSTYPE" == msys* || "$OSTYPE" == cygwin* || "$OSTYPE" == win32 ]] && PREVIOUS_BINARY_NAME="$PREVIOUS_BINARY_NAME.exe"
  [ -x "$CORE_JOURNEY_CLI_PATH" ] && [ "$(basename "$CORE_JOURNEY_CLI_PATH")" = "$PREVIOUS_BINARY_NAME" ] || {
    echo 'FAIL N-1 build did not produce its declared CLI binary at the selected instance target' >&2; exit 1;
  }
fi
dev_resolve || { echo "FAIL dev connection unresolved" >&2; exit 7; }
case "$EVAL_LIB_DEV_API_PORT" in
  7685|7686) echo "FAIL instance resolved to shared port $EVAL_LIB_DEV_API_PORT" >&2; exit 7 ;;
esac
[ -n "$EVAL_LIB_RESOLVED_INSTANCE" ] || { echo "FAIL resolved target has no instance name" >&2; exit 7; }

mkdir -p "$OUT" && OUT="$(cd "$OUT" && pwd)"
STARTED=$SECONDS

driver() { dev_run node "$HERE/driver.mjs" "$1" --out "$2" ${PASS[@]+"${PASS[@]}"} "${@:3}"; }
restart() { dev_run node "$HERE/restart.mjs" --out "$1" "${@:2}"; }

# Every stage runs even after a failure so the verdict covers all journeys; the worst exit wins.
cycle() {
  local out="$1" status=0
  shift
  driver setup "$out" || status=1
  dev_assert_restart_target 1 || return 7
  restart "$out" "$@" || status=1
  driver verify "$out" ${VERIFY_EXTRA[@]+"${VERIFY_EXTRA[@]}"} || status=1
  driver cleanup "$out" || status=1
  return "$status"
}

case "$MODE" in
  setup|verify|cleanup) driver "$MODE" "$OUT"; STATUS=$? ;;
  all) cycle "$OUT"; STATUS=$? ;;
  upgrade)
    STATUS=0
    UPGRADE_OUT="$OUT/n-1-to-n"
    UPGRADE_BASELINE="$UPGRADE_OUT/upgrade-baseline.json"
    mkdir -p "$UPGRADE_OUT"
    driver setup "$UPGRADE_OUT" --upgrade-baseline-out "$UPGRADE_BASELINE" || STATUS=1
    restart "$UPGRADE_OUT" --stop-in "$FROM" --start-in "$ROOT" --start dev --label upgrade || STATUS=1
    select_build_identity "$ROOT" || STATUS=1
    driver verify "$UPGRADE_OUT" --expect-build-change --upgrade-baseline "$UPGRADE_BASELINE" || STATUS=1
    restart "$UPGRADE_OUT" --stop-in "$ROOT" --start-in "$ROOT" --start dev --label upgrade-second-start || STATUS=1
    driver verify "$UPGRADE_OUT" --upgrade-baseline "$UPGRADE_BASELINE" --second-start || STATUS=1
    driver cleanup "$UPGRADE_OUT" || STATUS=1
    cycle "$OUT/n" || STATUS=1
    ;;
esac

echo "CORE JOURNEYS $MODE exit=$STATUS wall=$((SECONDS - STARTED))s out=$OUT"
exit "$STATUS"
