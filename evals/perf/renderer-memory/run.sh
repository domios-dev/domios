#!/usr/bin/env bash
# Renderer memory over a long session: several workspaces of busy streaming tabs,
# visited round-robin; one CDP sample per full cycle. Named dev instance only.
#
#   CODETERM_INSTANCE=perfqa CODETERM_API_PORT=7741 \
#     bash evals/perf/renderer-memory/run.sh [--workspaces 3] [--tabs 6] [--cycles 8] \
#       [--dwell-lines 200] [--pingpong 10] [--snapshots] [--keep] [--out DIR]
# Writes samples.jsonl (one CDP sample per cycle) and visits.jsonl (click-to-ready ms per visit).
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
ROOT="$(git -C "$HERE" rev-parse --show-toplevel)"
# shellcheck source=../../_lib/lib.sh
source "$ROOT/evals/_lib/lib.sh"

WORKSPACES=3
TABS=6
CYCLES=8
DWELL_LINES=200
PINGPONG=10
SNAPSHOTS=0
KEEP=0
OUT="$ROOT/target-check/evals/renderer-memory-$(date +%Y%m%d-%H%M%S)"
while [ $# -gt 0 ]; do
  case "$1" in
    --workspaces) WORKSPACES="$2"; shift 2 ;;
    --tabs) TABS="$2"; shift 2 ;;
    --cycles) CYCLES="$2"; shift 2 ;;
    --dwell-lines) DWELL_LINES="$2"; shift 2 ;;
    --pingpong) PINGPONG="$2"; shift 2 ;;
    --snapshots) SNAPSHOTS=1; shift ;;
    --keep) KEEP=1; shift ;;
    --out) OUT="$2"; shift 2 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done

if [ -z "${CODETERM_INSTANCE:-}" ]; then
  echo "ERROR: run against a named dev instance (CODETERM_INSTANCE + CODETERM_API_PORT)." >&2
  exit 7
fi
dev_assert_restart_target || exit 7
IDENTITY="$(dc identify --json)"
printf '%s' "$IDENTITY" | python3 -c '
import json, os, sys
c = json.load(sys.stdin)["connection"]
want = (os.environ["CODETERM_INSTANCE"], int(os.environ["CODETERM_API_PORT"]))
got = (c.get("instance"), c.get("port"))
if got != want or got[1] in (7685, 7686):
    sys.exit(f"identify mismatch: want {want}, got {got}")
print(f"target verified: instance={got[0]} port={got[1]}")
'
CDP="$(dev_cdp_port)"
VITE="$(dev_vite_port)"
mkdir -p "$OUT"
SAMPLES="$OUT/samples.jsonl"
VISITS="$OUT/visits.jsonl"
: > "$SAMPLES"
: > "$VISITS"

STREAM_CMD="$OUT/stream-cmd.txt"
STREAM_JS="$HERE/stream.mjs"
is_windows && STREAM_JS="$(cygpath -w "$STREAM_JS")"
printf 'node "%s"\n' "$STREAM_JS" > "$STREAM_CMD"

WS_IDS=()
TAB_IDS=()
cleanup() {
  [ "$KEEP" = 1 ] && return
  for tab in "${TAB_IDS[@]}"; do dc tab close "$tab" >/dev/null 2>&1 || true; done
  for ws in "${WS_IDS[@]}"; do dc workspace close "$ws" >/dev/null 2>&1 || true; done
}
trap cleanup EXIT

for w in $(seq 1 "$WORKSPACES"); do
  cwd="$OUT/ws$w"
  ws="$(new_workspace "perfqa-mem-$w" "$cwd")"
  [ -n "$ws" ] || { echo "workspace create failed" >&2; exit 1; }
  WS_IDS+=("$ws")
  for t in $(seq 1 "$TABS"); do
    tab="$(dc tab new --workspace "$ws" --cwd "$cwd" --title "s$w$t" --eager --json       | python3 -c "import json,sys; print(json.load(sys.stdin).get('tab_id',''))")"
    [ -n "$tab" ] || { echo "tab create failed" >&2; exit 1; }
    TAB_IDS+=("$tab")
    dc send --tab "$tab" --to-shell --from-file "$STREAM_CMD" >/dev/null
  done
done
echo "created ${#WS_IDS[@]} workspaces, ${#TAB_IDS[@]} streaming tabs"

sample() {
  local label="$1" extra=()
  [ "$SNAPSHOTS" = 1 ] && [ "${2:-}" = snapshot ] && extra=(--snapshot "$OUT/$label.heapsnapshot")
  node "$HERE/sample.mjs" --cdp "$CDP" --vite "$VITE" --label "$label" "${extra[@]}" | tee -a "$SAMPLES"
}

visit() {
  local phase="$1" tab="$2"
  node "$HERE/visit.mjs" --cdp "$CDP" --vite "$VITE" --tab "$tab" --dwell-lines "$DWELL_LINES" --deadline-ms 30000     | python3 -c "import json,sys; v=json.load(sys.stdin); v['phase']='$phase'; print(json.dumps(v))" >> "$VISITS"
}

visit start "${TAB_IDS[0]}"
sample t0 snapshot
for c in $(seq 1 "$CYCLES"); do
  for tab in "${TAB_IDS[@]}"; do visit "cycle$c" "$tab"; done
  if [ "$c" = "$CYCLES" ]; then sample "cycle$c" snapshot; else sample "cycle$c"; fi
done
for _ in $(seq 1 "$PINGPONG"); do
  visit pingpong "${TAB_IDS[0]}"
  visit pingpong "${TAB_IDS[1]}"
done
echo "samples: $SAMPLES"
echo "visits: $VISITS"
