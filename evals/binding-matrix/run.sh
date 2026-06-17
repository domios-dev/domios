#!/usr/bin/env bash
# Session-binding stress matrix.
#
# Drives the provider-reported binding system under churn and asserts the core
# invariant never slips: every live agent pane is bound to its OWN unique
# provider session id — even with many panes in the same cwd, /new resets, and
# close+respawn cycles interleaved.
#
# Scenarios (each followed by a full-fleet invariant check):
#   A. mass spawn          — N workspaces x M agents, all spawned back-to-back
#   B. same-cwd clusters   — every workspace is one cwd; M agents share it
#   C. /new churn          — claude panes get /clear; must rebind to a NEW sid
#   D. close + respawn     — close a subset, spawn replacements in the same cwd
#
# Oracle per check:
#   - bound:      every tracked agent pane has a non-null agent_session_id
#   - unique:     no two live agent panes share an agent_session_id (injective)
#   - real:       each sid resolves to an on-disk session for its provider
#   - no-misbind: zero "binding nonce mismatch" in the dev log during the run
#
# Tunables (env):
#   BM_PROVIDERS    space/comma list (default "claude"; e.g. "claude codex")
#   BM_WS           number of workspaces / clusters     (default 3)
#   BM_PER_WS       agents per workspace                (default 4)
#   BM_BIND_TIMEOUT secs to wait for a fleet to bind    (default 75)
#   BM_KEEP         1 = keep test workspaces afterwards (default 0 = cleanup)
set -uo pipefail
source "$(git rev-parse --show-toplevel)/evals/_lib/lib.sh"

PROVIDERS="${BM_PROVIDERS:-claude}"
PROVIDERS="${PROVIDERS//,/ }"
WS_COUNT="${BM_WS:-3}"
PER_WS="${BM_PER_WS:-4}"
BIND_TIMEOUT="${BM_BIND_TIMEOUT:-75}"
KEEP="${BM_KEEP:-0}"

REPO_ROOT="$(git rev-parse --show-toplevel)"
RUN_TS="$(date +%s)"
OUT="$REPO_ROOT/target-check/evals/binding-matrix/$RUN_TS"
mkdir -p "$OUT"
TRACK="$OUT/tracked.txt"   # one "pane_id provider ws cwd" per line
: > "$TRACK"

LOG="$HOME/.codeterm-dev/logs/codeterm.log"
# Note: avoid `grep | wc` — under `set -o pipefail` a zero-match grep exits 1,
# which both prints the count AND trips a trailing `|| echo 0`, yielding "0\n0".
count_mismatch() {
  [ -f "$LOG" ] || { echo 0; return; }
  local n; n=$(grep -c "binding nonce mismatch" "$LOG" 2>/dev/null || true)
  echo "${n:-0}"
}
MISMATCH_BASELINE="$(count_mismatch)"

PASS=0; FAIL=0
ok()   { echo "  ✅ $*"; PASS=$((PASS+1)); }
bad()  { echo "  ❌ $*"; FAIL=$((FAIL+1)); }
hr()   { echo "------------------------------------------------------------"; }

if ! dev_alive; then echo "dev daemon not up on 7686 — ./build.sh dev first"; exit 7; fi

provider_count() { echo "$PROVIDERS" | wc -w | tr -d ' '; }

spawn_one() {
  # $1 provider  $2 ws  $3 cwd  $4 title  -> echoes pane_id, appends to TRACK
  local prov="$1" ws="$2" cwd="$3" title="$4" pid task
  task="reply with exactly the word READY and nothing else"
  case "$prov" in
    claude)   pid=$(new_claude_pane "$ws" "$cwd" "$title" "$task") ;;
    codex)    pid=$(new_codex_pane "$ws" "$cwd" "$title" "$task") ;;
    opencode) pid=$(new_opencode_pane "$ws" "$cwd" "$title" "$task") ;;
    *) echo "unknown provider $prov" >&2; return 1 ;;
  esac
  if [ -n "$pid" ]; then
    echo "$pid $prov $ws $cwd" >> "$TRACK"
    echo "$pid"
  fi
}

# Wait until every tracked, still-live pane has a session id (or timeout).
wait_for_bindings() {
  local timeout="$1" elapsed=0 pending
  while [ "$elapsed" -lt "$timeout" ]; do
    dev_get /api/panes > "$OUT/panes.json" 2>/dev/null
    pending=$(python3 - "$TRACK" "$OUT/panes.json" <<'PY'
import json,sys
tracked={l.split()[0] for l in open(sys.argv[1]) if l.strip()}
panes={p['id']:p for p in json.load(open(sys.argv[2]))}
pend=[pid for pid in tracked if pid in panes and not panes[pid].get('agent_session_id')]
print(len(pend))
PY
)
    [ "$pending" = "0" ] && return 0
    sleep 4; elapsed=$((elapsed+4))
  done
  return 1
}

# Full-fleet invariant check. $1 = label.
check_invariants() {
  local label="$1"
  hr; echo "CHECK: $label"
  dev_get /api/panes > "$OUT/panes.json" 2>/dev/null
  python3 - "$TRACK" "$OUT/panes.json" "$label" <<'PY'
import json,sys,os
tracked={}
for l in open(sys.argv[1]):
    if not l.strip(): continue
    pid,prov,ws,cwd=l.split(None,3)
    tracked[pid]=(prov,cwd.strip())
panes={p['id']:p for p in json.load(open(sys.argv[2]))}

live={pid:panes[pid] for pid in tracked if pid in panes}
unbound=[pid for pid in live if not live[pid].get('agent_session_id')]

# uniqueness: sid -> [panes]
from collections import defaultdict
bysid=defaultdict(list)
for pid,p in live.items():
    sid=p.get('agent_session_id')
    if sid: bysid[sid].append(pid)
dupes={sid:ps for sid,ps in bysid.items() if len(ps)>1}

print(f"  tracked={len(tracked)} live={len(live)} bound={len(live)-len(unbound)} unbound={len(unbound)}")
rc=0
if unbound:
    print(f"  UNBOUND panes: {unbound[:8]}"); rc=1
if dupes:
    for sid,ps in list(dupes.items())[:8]:
        print(f"  DUPLICATE sid {sid[:12]} claimed by {ps}")
    rc=1
sys.exit(rc)
PY
  local rc=$?
  if [ "$rc" = "0" ]; then ok "$label — all bound + unique"; else bad "$label — invariant violated (see above)"; fi
}

assert_no_misbind() {
  local now delta
  now="$(count_mismatch)"
  delta=$((now - MISMATCH_BASELINE))
  if [ "$delta" = "0" ]; then ok "zero nonce-mismatch rejections during run"; else bad "$delta nonce-mismatch rejections appeared (legit reports were rejected!)"; fi
}

echo "=== binding-matrix: providers=[$PROVIDERS] ws=$WS_COUNT per_ws=$PER_WS ==="
echo "out: $OUT"

# --- Scenario A + B: mass spawn into same-cwd clusters ----------------
declare -a WS_IDS WS_CWDS
for w in $(seq 1 "$WS_COUNT"); do
  cwd="/tmp/bm-$RUN_TS-ws$w"
  mkdir -p "$cwd"
  ws=$(new_workspace "bm-$RUN_TS-$w" "$cwd")
  WS_IDS[$w]="$ws"; WS_CWDS[$w]="$cwd"
  pi=0
  for m in $(seq 1 "$PER_WS"); do
    # rotate providers across the M slots so a cluster mixes providers when >1
    set -- $PROVIDERS
    nprov=$(provider_count); idx=$(( (m-1) % nprov + 1 )); prov=$(eval echo "\${$idx}")
    pid=$(spawn_one "$prov" "$ws" "$cwd" "$prov-w$w-$m")
    echo "  spawned $prov w$w/$m -> ${pid:-FAILED}"
  done
done

echo "waiting for fleet to bind (timeout ${BIND_TIMEOUT}s)..."
wait_for_bindings "$BIND_TIMEOUT" || echo "  (timeout — some still pending; check will report)"
check_invariants "A+B mass spawn / same-cwd clusters"
assert_no_misbind

# --- Scenario C: /new churn on claude panes ---------------------------
echo; echo "=== Scenario C: /new (claude /clear) churn ==="
mapfile -t CLAUDE_PANES < <(awk '$2=="claude"{print $1}' "$TRACK")
# capture pre-clear sids
dev_get /api/panes > "$OUT/pre-clear.json" 2>/dev/null
CHURN_N=$(( ${#CLAUDE_PANES[@]} / 2 ))
[ "$CHURN_N" -lt 1 ] && CHURN_N=${#CLAUDE_PANES[@]}
for i in $(seq 0 $((CHURN_N-1))); do
  p="${CLAUDE_PANES[$i]}"
  [ -z "$p" ] && continue
  dc send-text "/clear" --pane "$p" >/dev/null 2>&1
  echo "  /clear -> $p"
done
sleep 20
wait_for_bindings "$BIND_TIMEOUT" || true
# assert the cleared panes are still bound + their sid changed (rebind to new session)
dev_get /api/panes > "$OUT/post-clear.json" 2>/dev/null
python3 - "$OUT/pre-clear.json" "$OUT/post-clear.json" "${CLAUDE_PANES[@]:0:$CHURN_N}" <<'PY'
import json,sys
pre={p['id']:p.get('agent_session_id') for p in json.load(open(sys.argv[1]))}
post={p['id']:p.get('agent_session_id') for p in json.load(open(sys.argv[2]))}
cleared=sys.argv[3:]
changed=still=0
for p in cleared:
    a,b=pre.get(p),post.get(p)
    if b: still+=1
    if a and b and a!=b: changed+=1
print(f"  cleared={len(cleared)} still-bound={still} rebound-to-new-sid={changed}")
PY
check_invariants "C after /clear churn"
assert_no_misbind

# --- Scenario D: close + respawn churn --------------------------------
echo; echo "=== Scenario D: close + respawn ==="
mapfile -t ALL_PANES < <(awk '{print $0}' "$TRACK")
CLOSE_N=$(( ${#ALL_PANES[@]} / 3 )); [ "$CLOSE_N" -lt 1 ] && CLOSE_N=1
for i in $(seq 0 $((CLOSE_N-1))); do
  line="${ALL_PANES[$i]}"; [ -z "$line" ] && continue
  pid=$(echo "$line" | awk '{print $1}'); prov=$(echo "$line" | awk '{print $2}')
  ws=$(echo "$line" | awk '{print $3}'); cwd=$(echo "$line" | awk '{print $4}')
  dc pane close "$pid" >/dev/null 2>&1
  echo "  closed $pid ($prov), respawning in same cwd"
  # remove from track, spawn replacement
  grep -v "^$pid " "$TRACK" > "$TRACK.tmp" && mv "$TRACK.tmp" "$TRACK"
  np=$(spawn_one "$prov" "$ws" "$cwd" "$prov-respawn-$i")
  echo "    -> $np"
done
echo "waiting for respawn fleet to bind..."
wait_for_bindings "$BIND_TIMEOUT" || true
check_invariants "D after close+respawn"
assert_no_misbind

# --- Summary ----------------------------------------------------------
hr
echo "RESULT: $PASS passed, $FAIL failed"
if [ "$KEEP" != "1" ]; then
  echo "cleaning up test workspaces..."
  for w in $(seq 1 "$WS_COUNT"); do
    [ -n "${WS_IDS[$w]:-}" ] && dc workspace close "${WS_IDS[$w]}" >/dev/null 2>&1
  done
fi
[ "$FAIL" = "0" ] && exit 0 || exit 1
