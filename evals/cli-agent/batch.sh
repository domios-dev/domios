#!/usr/bin/env bash
# One self-contained benchmark batch (≤8 cases, capped concurrency).
# Usage: batch.sh <provider> <runs_dir> <batch_file>
#   batch_file = newline-separated case ids (must exist in cases/*.yaml)
# Cleans bench workspaces before AND after; dumps <cid>.json/.md to runs_dir;
# prints a compact summary the model reads to judge.
set -uo pipefail
HARNESS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=/dev/null
source "$HARNESS_DIR/lib.sh"

PROVIDER="$1"; RUNS="$2"; BATCH_FILE="$3"; MODEL="${4:-}"
mkdir -p "$RUNS"
SBROOT="$(mktemp -d /tmp/codeterm-bench-XXXXXX)"   # OUTSIDE the repo: agents must get the generated AGENTS.md (CLI prompt), not the repo dev instructions
TABS="$RUNS/.tabs-$(basename "$BATCH_FILE").tsv"; : > "$TABS"

cleanup_bench_ws   # clean any leftovers first
trap cleanup_bench_ws EXIT   # ALWAYS clean on exit, even if the batch errors mid-poll

# id -> prompt map from all case files
prompt_for() {  # <cid>
  python3 - "$HARNESS_DIR/cases" "$1" <<'PY'
import sys, glob, os, yaml
for f in glob.glob(os.path.join(sys.argv[1], "*.yaml")):
    for c in (yaml.safe_load(open(f)) or []):
        if c.get("id") == sys.argv[2]:
            print(c.get("prompt","")); sys.exit(0)
PY
}

echo "── batch $(basename "$BATCH_FILE") on $PROVIDER ──"
while read -r CID; do
  [ -n "$CID" ] || continue
  PROMPT="$(prompt_for "$CID")"
  [ -n "$PROMPT" ] || { echo "  ! no prompt for $CID"; continue; }
  (
    SB="$SBROOT/$CID"; mkdir -p "$SB"
    WS=$(dc workspace create "bench-$CID" --cwd "$SB" --json 2>/dev/null | python3 -c 'import sys,json;print(json.load(sys.stdin).get("id",""))')
    TAB=$(dc agent spawn "$PROVIDER" ${MODEL:+--model "$MODEL"} --workspace "$WS" --cwd "$SB" --title "$CID" --json 2>/dev/null | python3 -c 'import sys,json;print(json.load(sys.stdin).get("tab_id",""))')
    sleep 8
    [ -n "$TAB" ] && dc send "$PROMPT" --tab "$TAB" >/dev/null 2>&1
    printf '%s\t%s\t%s\t%s\n' "$CID" "$WS" "$TAB" "$PROMPT" >> "$TABS"
  ) </dev/null &
done < "$BATCH_FILE"
wait
echo "  spawned $(wc -l < "$TABS") agents; polling…"

# poll all tabs until idle (count stable ≥3 rounds) or ~150s
declare -A prev stable
for round in $(seq 1 50); do
  alldone=1
  while IFS=$'\t' read -r CID WS TAB PROMPT; do
    [ -n "$TAB" ] || continue
    c=$(dc tab chat --tab "$TAB" --json 2>/dev/null | python3 -c 'import sys,json;print(json.load(sys.stdin).get("count",0))' 2>/dev/null || echo 0)
    if [ "$c" = "${prev[$CID]:-x}" ] && [ "$c" -gt 0 ]; then stable[$CID]=$(( ${stable[$CID]:-0}+1 )); else stable[$CID]=0; fi
    prev[$CID]=$c
    [ "${stable[$CID]:-0}" -ge 3 ] || alldone=0
  done < "$TABS"
  [ "$alldone" = 1 ] && break
  sleep 3
done
echo "  settled ~$((round*3))s"

# dump transcripts + summary
echo "── SUMMARY (cid | count | first tool calls) ──"
while IFS=$'\t' read -r CID WS TAB PROMPT; do
  [ -n "$TAB" ] || continue
  dc tab chat --tab "$TAB" --json > "$RUNS/$CID.json" 2>/dev/null || true
  python3 evals/cli-agent/transcript-md.py "$RUNS/$CID.json" "$CID" "$PROVIDER" "$PROMPT" > "$RUNS/$CID.md" 2>/dev/null || true
  python3 - "$CID" "$RUNS/$CID.json" <<'PY'
import sys,json
cid=sys.argv[1]
try: d=json.load(open(sys.argv[2]))
except: d={"count":0,"messages":[]}
cmds=[((m.get("toolName") or "?")+":"+((m.get("toolInput") or {}).get("command","") or "")[:46]) for m in d.get("messages",[]) if m.get("type")=="tool_call"]
print(f"  {cid:26} | {d.get('count',0):3} | " + (" || ".join(cmds[:3]) if cmds else "(no tool calls)"))
PY
done < "$TABS"

cleanup_bench_ws   # clean after
rm -rf "$SBROOT"
echo "── batch done; workspaces cleaned ──"
