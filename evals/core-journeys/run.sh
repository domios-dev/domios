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
  upgrade   J5: --from-checkout N-1_CHECKOUT on a named disposable CODETERM_INSTANCE
Driver flags: --providers a,b  --history-kb N  --deadline-secs N  --all-tabs  --keep
Release gate: see evals/core-journeys/README.md. Spends real provider tokens.
EOF
}

if [ "${1:-}" = --dc ]; then
  shift
  unset CODETERM_CLI_PATH
  source "$ROOT/evals/_lib/lib.sh"
  if dc "$@"; then exit 0; else exit "$?"; fi
fi
case "${1:-}" in
  all|setup|verify|cleanup|upgrade) MODE="$1"; shift ;;
  -h|--help|"") usage; exit 0 ;;
  *) echo "Unknown mode: $1" >&2; usage >&2; exit 2 ;;
esac

OUT="" FROM="" PASS=() VERIFY_EXTRA=()
while [ "$#" -gt 0 ]; do
  case "$1" in
    --out) OUT="${2:?--out needs a directory}"; shift 2 ;;
    --from-checkout) FROM="${2:?--from-checkout needs a path}"; shift 2 ;;
    *) PASS+=("$1"); shift ;;
  esac
done

for prerequisite in node python3 git; do
  command -v "$prerequisite" >/dev/null || { echo "FAIL missing prerequisite: $prerequisite" >&2; exit 7; }
done
if [ "$MODE" = upgrade ]; then
  case "${CODETERM_INSTANCE:-}:${CODETERM_API_PORT:-}" in
    :*|*:|*:7685|*:7686) echo "FAIL upgrade needs a named disposable CODETERM_INSTANCE and its own CODETERM_API_PORT" >&2; exit 2 ;;
  esac
  [ -n "$FROM" ] && [ -x "$FROM/build.sh" ] || { echo "FAIL --from-checkout must name the build N-1 checkout" >&2; exit 2; }
  FROM="$(cd "$FROM" && pwd)"
  # The disposable instance has no CLI before its first build, so build N-1 starts before the lib resolves it.
  (cd "$FROM" && bash ./build.sh dev --wait --wait-build) || { echo "FAIL build N-1 did not start" >&2; exit 1; }
fi
unset CODETERM_CLI_PATH
source "$ROOT/evals/_lib/lib.sh"
dev_resolve || { echo "FAIL dev connection unresolved" >&2; exit 7; }
[ "$EVAL_LIB_DEV_API_PORT" != 7685 ] || { echo "FAIL refusing production port 7685" >&2; exit 7; }

OUT="${OUT:-$ROOT/target-check/evals/core-journeys/$(date +%Y%m%d-%H%M%S)-$$}"
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
    VERIFY_EXTRA=(--expect-build-change)
    cycle "$OUT/n-1-to-n" --stop-in "$FROM" --start-in "$ROOT" --start dev --label upgrade || STATUS=1
    VERIFY_EXTRA=()
    cycle "$OUT/n" || STATUS=1
    ;;
esac

echo "CORE JOURNEYS $MODE exit=$STATUS wall=$((SECONDS - STARTED))s out=$OUT"
exit "$STATUS"
