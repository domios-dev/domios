#!/usr/bin/env bash
# Session-binding stress matrix.
#
# Drives the provider-reported binding system under churn and asserts the core
# invariant never slips: every live agent tab is bound to its OWN unique
# provider session id — even with many tabs in the same cwd, /new resets, and
# close+respawn cycles interleaved.
#
# Scenarios (each followed by a full-fleet invariant check):
#   A. mass spawn          — N workspaces x M agents, all spawned back-to-back
#   B. same-cwd clusters   — every workspace is one cwd; M agents share it
#   C. /new churn          — claude tabs get /clear; must rebind to a NEW sid
#   D. close + respawn     — close a subset, spawn replacements in the same cwd
#
# Oracle per check:
#   - bound:      every tracked agent tab has a non-null agent_session_id
#   - unique:     no two live agent tabs share an agent_session_id (injective)
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
REPO_ROOT="$(git rev-parse --show-toplevel)"
source "$REPO_ROOT/evals/_lib/bench.sh"
bench_require python3 node curl
source "$REPO_ROOT/evals/_lib/lib.sh"
source "$(git rev-parse --show-toplevel)/evals/_lib/role_preset.sh"

PROVIDERS="${BM_PROVIDERS:-claude}"
PROVIDERS="${PROVIDERS//,/ }"
WS_COUNT="${BM_WS:-3}"
PER_WS="${BM_PER_WS:-4}"
BIND_TIMEOUT="${BM_BIND_TIMEOUT:-75}"
KEEP="${BM_KEEP:-0}"
[[ "$WS_COUNT" =~ ^[1-9][0-9]*$ ]] && [[ "$PER_WS" =~ ^[1-9][0-9]*$ ]] && [ -n "$PROVIDERS" ] || { echo "FAIL empty or invalid binding fixture" >&2; exit 1; }

REPO_ROOT="$(git rev-parse --show-toplevel)"
RUN_TS="$(date +%s)"
OUT="$REPO_ROOT/target-check/evals/binding-matrix/$RUN_TS"
mkdir -p "$OUT"
TRACK="$OUT/tracked.txt"   # one "tab_id provider ws cwd" per line
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
ok()   { bench_assertion; echo "  ✅ $*"; PASS=$((PASS+1)); }
bad()  { bench_assertion; echo "  ❌ $*"; FAIL=$((FAIL+1)); }
hr()   { echo "------------------------------------------------------------"; }

if ! dev_alive; then echo "dev daemon not up on 7686 — ./build.sh dev first"; exit 7; fi

# Binding churn must not let a rebound tab pick up a different preset: the
# resolver is keyed by kind, not by session or provider identity.
if assert_role_preset_seam; then
  ok "role preset seam intact before binding churn"
else
  bad "role preset seam drifted before binding churn"
fi

provider_count() { echo "$PROVIDERS" | wc -w | tr -d ' '; }

spawn_one() {
  # $1 provider  $2 ws  $3 cwd  $4 title  -> echoes tab_id, appends to TRACK
  local prov="$1" ws="$2" cwd="$3" title="$4" pid task
  task="reply with exactly the word READY and nothing else"
  case "$prov" in
    claude)   pid=$(new_claude_tab "$ws" "$cwd" "$title" "$task") ;;
    codex)    pid=$(new_codex_tab "$ws" "$cwd" "$title" "$task") ;;
    opencode) pid=$(new_opencode_tab "$ws" "$cwd" "$title" "$task") ;;
    *) echo "unknown provider $prov" >&2; return 1 ;;
  esac
  if [ -n "$pid" ]; then
    echo "$pid $prov $ws $cwd" >> "$TRACK"
    echo "$pid"
  fi
}

# Wait until every tracked, still-live tab has a session id (or timeout).
wait_for_bindings() {
  local timeout="$1"
  shift
  dev_run node "$REPO_ROOT/evals/binding-matrix/wait-bindings.mjs" "$timeout" "$TRACK" "$OUT/tabs.json" "$@"
}

# Full-fleet invariant check. $1 = label.
check_invariants() {
  local label="$1"
  hr; echo "CHECK: $label"
  dev_get /api/tabs > "$OUT/tabs.json" 2>/dev/null
  python3 "$REPO_ROOT/evals/binding-matrix/check_fleet.py" "$TRACK" "$OUT/tabs.json" "$label"
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

[ "$(wc -l < "$TRACK")" -eq "$((WS_COUNT * PER_WS))" ] || { echo "FAIL incomplete spawned fleet" >&2; exit 1; }
echo "waiting for fleet to bind (timeout ${BIND_TIMEOUT}s)..."
wait_for_bindings "$BIND_TIMEOUT" || echo "  (timeout — some still pending; check will report)"
check_invariants "A+B mass spawn / same-cwd clusters"
assert_no_misbind

# --- Scenario C: /new churn on claude tabs ---------------------------
echo; echo "=== Scenario C: /new (claude /clear) churn ==="
CLAUDE_TABS=()
CLAUDE_COUNT=0
while IFS= read -r tab; do
  CLAUDE_TABS+=("$tab")
  CLAUDE_COUNT=$((CLAUDE_COUNT + 1))
done < <(awk '$2=="claude"{print $1}' "$TRACK")
if [ "$CLAUDE_COUNT" -gt 0 ]; then
# capture pre-clear sids
dev_get /api/tabs > "$OUT/pre-clear.json" 2>/dev/null
CHURN_N=$(( $CLAUDE_COUNT / 2 ))
[ "$CHURN_N" -lt 1 ] && CHURN_N=$CLAUDE_COUNT
for i in $(seq 0 $((CHURN_N-1))); do
  p="${CLAUDE_TABS[$i]}"
  [ -z "$p" ] && continue
  dc send "/clear" --tab "$p" >/dev/null 2>&1
  echo "  /clear -> $p"
done
wait_for_bindings "$BIND_TIMEOUT" "$OUT/pre-clear.json" "${CLAUDE_TABS[@]:0:$CHURN_N}" || true
# assert the cleared tabs are still bound + their sid changed (rebind to new session)
dev_get /api/tabs > "$OUT/post-clear.json" 2>/dev/null
python3 - "$OUT/pre-clear.json" "$OUT/post-clear.json" "${CLAUDE_TABS[@]:0:$CHURN_N}" <<'PY'
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
sys.exit(0 if changed == len(cleared) else 1)
PY
if [ "$?" -eq 0 ]; then ok "cleared tabs rebound"; else bad "cleared tabs failed to rebind"; fi
check_invariants "C after /clear churn"
assert_no_misbind
else
  echo "SKIP Claude /clear churn: no Claude tabs requested"
fi

# --- Scenario D: close + respawn churn --------------------------------
echo; echo "=== Scenario D: close + respawn ==="
ALL_TABS=()
ALL_COUNT=0
while IFS= read -r line; do
  ALL_TABS+=("$line")
  ALL_COUNT=$((ALL_COUNT + 1))
done < "$TRACK"
[ "$ALL_COUNT" -gt 0 ] || { echo "FAIL empty fleet before respawn" >&2; exit 1; }
CLOSE_N=$(( $ALL_COUNT / 3 )); [ "$CLOSE_N" -lt 1 ] && CLOSE_N=1
for i in $(seq 0 $((CLOSE_N-1))); do
  line="${ALL_TABS[$i]}"; [ -z "$line" ] && continue
  pid=$(echo "$line" | awk '{print $1}'); prov=$(echo "$line" | awk '{print $2}')
  ws=$(echo "$line" | awk '{print $3}'); cwd=$(echo "$line" | awk '{print $4}')
  dc tab close "$pid" >/dev/null 2>&1
  echo "  closed $pid ($prov), respawning in same cwd"
  # remove from track, spawn replacement
  awk -v pid="$pid" '$1 != pid' "$TRACK" > "$TRACK.tmp" && mv "$TRACK.tmp" "$TRACK"
  np=$(spawn_one "$prov" "$ws" "$cwd" "$prov-respawn-$i")
  echo "    -> $np"
done
[ "$(wc -l < "$TRACK")" -eq "$((WS_COUNT * PER_WS))" ] || { echo "FAIL incomplete respawned fleet" >&2; exit 1; }
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
bench_complete
[ "$FAIL" = "0" ] && exit 0 || exit 1
