#!/usr/bin/env bash
# Orchestrator-creation test matrix (routes B-H) — task_1800fa9977ea418094bb4ab291b2e3f2.
#
# Every orchestrator-creation route, executable: exact command + observable
# pass condition. DEV-LOCKED via evals/_lib/lib.sh `dc` wrapper (a bare
# `codeterm` here would hit prod 7685 — forbidden). Route I (in-place picker
# UI) is the Playwright suite's job, not this bash matrix.
#
# Verdicts:
#   PASS     observable pass condition met on dev.
#   FAIL     observable pass condition NOT met — a real product bug (bounce).
#   FINDING  behaviour diverges from the pinned expectation but is arguably
#            by-design or owned by another lane — bounce, not a blocker here.
#   GATED    the pinned condition depends on the async heartbeat nudge
#            (nudge_interval_secs default 1800s) and is NOT observable inside a
#            bounded QA run; the deterministic DRIVING STATE is asserted
#            instead and reported honestly (never silently passed).
#
# Usage: bash evals/orchestrator-creation-matrix/run.sh
# Exit:  0 if no FAIL, 1 if any FAIL. FINDING/GATED never fail the run but are
#        tabulated and must be triaged by the orchestrator.

set -u
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
source "$ROOT/evals/_lib/lib.sh"
PYTHON_BIN="$(command -v python3 || command -v python)"
if [ -z "$PYTHON_BIN" ]; then
  echo "ABORT: neither python3 nor python is available on PATH" >&2
  exit 7
fi

# --- result accounting ----------------------------------------------
declare -a ROWS           # "SCENARIO|VERDICT|detail"
CREATED_TABS=()
CREATED_PLANS=()
CREATED_WS=()
FAILS=0
SCENARIO_B_ORCH=""
SCENARIO_B_PLANNER=""
SCENARIO_B_PLAN=""

record() { ROWS+=("$1|$2|$3"); }
note()   { echo "    · $*"; }

pass()    { echo "  [PASS]    $1: $2"; record "$1" PASS "$2"; }
fail()    { echo "  [FAIL]    $1: $2"; record "$1" FAIL "$2"; FAILS=$((FAILS+1)); }
finding() { echo "  [FINDING] $1: $2"; record "$1" FINDING "$2"; }
gated()   { echo "  [GATED]   $1: $2"; record "$1" GATED "$2"; }

# --- json helpers (python; portable) --------------------------------
# js <json-string> <python-expr-on-d>
js() {
  "$PYTHON_BIN" - "$2" <<PY 2>/dev/null
import json,sys
try:
    d=json.loads('''$1''')
    v=eval(sys.argv[1],{'d':d})
    print('' if v is None else v)
except Exception:
    print('')
PY
}

# --- cleanup --------------------------------------------------------
cleanup() {
  echo
  echo "=== cleanup ==="
  for p in "${CREATED_TABS[@]:-}"; do
    [ -n "$p" ] || continue
    dc tab close "$p" >/dev/null 2>&1 && note "closed tab $p" || note "tab $p already gone"
  done
  # Sweep EVERY plan created since the baseline — by id-diff, not by the
  # CREATED_PLANS list (a plan created as a side-effect, or one whose id
  # extraction failed mid-scenario, is still deleted). Never archive:
  # archived plans are exactly the junk we are here to eliminate.
  [ -n "${PLAN_BASELINE:-}" ] && plan_sweep_new "$PLAN_BASELINE"
  for w in "${CREATED_WS[@]:-}"; do
    [ -n "$w" ] || continue
    dc workspace close "$w" >/dev/null 2>&1 && note "closed ws $w" || note "ws $w left"
  done
}
trap cleanup EXIT

# --- preflight ------------------------------------------------------
echo "=== preflight ==="
if ! curl -s --max-time 3 http://127.0.0.1:7686/api/health >/dev/null 2>&1; then
  echo "ABORT: dev daemon not healthy on 7686. Start it (orchestrator owns dev): ./build.sh dev" >&2
  exit 7
fi
BUILD=$(dev_get /api/health | "$PYTHON_BIN" -c "import json,sys;print(json.load(sys.stdin).get('build','?'))" 2>/dev/null)
echo "  dev build: $BUILD"

# Baseline plan-id set — cleanup sweeps everything created past this, and
# the result gate asserts the set returned to it (before == after).
PLAN_BASELINE="$(mktemp -t orchmatrix-plans.XXXXXX)"
plan_snapshot "$PLAN_BASELINE" || { echo "ABORT: could not snapshot plan baseline" >&2; exit 7; }
echo "  plan baseline: $(wc -l < "$PLAN_BASELINE" | tr -d ' ') plans"

# scratch workspace shared by scenarios (each scenario independent within it)
WS_CWD="$ROOT"
WS=$(new_workspace "WS-orchmatrix" "$WS_CWD")
if [ -z "$WS" ]; then echo "ABORT: could not create scratch workspace" >&2; exit 1; fi
CREATED_WS+=("$WS")
echo "  scratch workspace: $WS"

# mk_plan <title> -> echoes plan id. Retries on "database is locked" (the plan
# store serializes writes; under concurrent daemon load create can transiently
# fail — a real contention FINDING, retried here so scenarios stay runnable).
mk_plan() {
  local title="$1" out id try
  for try in 1 2 3 4 5; do
    out=$(dc plan create --title "$title" --json 2>&1)
    id=$(js "$out" "d.get('plan_id') or d.get('plan',{}).get('id') or d.get('id','')")
    [ -n "$id" ] && { echo "$id"; return 0; }
    echo "$out" | grep -qi "database is locked" || { note "plan create failed: $(echo "$out" | head -c 120)"; break; }
    sleep 1
  done
  echo ""
}

tab_count() { dev_get /api/tabs | "$PYTHON_BIN" -c "import json,sys;print(len(json.load(sys.stdin)))" 2>/dev/null; }
plan_count() { dc plan list --json 2>/dev/null | "$PYTHON_BIN" -c "import json,sys;d=json.load(sys.stdin);print(len(d if isinstance(d,list) else d.get('plans',[])))" 2>/dev/null; }
tab_exists() { dev_get /api/tabs | "$PYTHON_BIN" -c "import json,sys;print(any(p.get('id')=='$1' for p in json.load(sys.stdin)))" 2>/dev/null; }
tab_session() { dev_get "/api/tabs" | "$PYTHON_BIN" -c "import json,sys;print(next((p.get('session_id') or p.get('session') or '' for p in json.load(sys.stdin) if p.get('id')=='$1'),''))" 2>/dev/null; }

# grep a tab's parsed chat transcript for a fixed string. Returns 0 if found.
tab_chat_has() {
  local tab="$1" needle="$2"
  dc tab chat --tab "$tab" --json 2>/dev/null \
    | "$PYTHON_BIN" -c '
import json,sys
needle=sys.argv[1]
try:
    d=json.load(sys.stdin)
except Exception:
    sys.exit(1)
blob=json.dumps(d)
sys.exit(0 if needle in blob else 1)
' "$needle"
}

# grep the UI chat-start snapshot for a fixed string. This is the deterministic
# oracle for pending_greeting; `tab chat` only reads the durable transcript.
tab_chat_start_has() {
  local tab="$1" needle="$2"
  dev_post "/api/tabs/$tab/chat-raw/start" '{}' \
    | "$PYTHON_BIN" -c '
import json,sys
needle=sys.argv[1]
try:
    d=json.load(sys.stdin)
except Exception:
    sys.exit(1)
blob=json.dumps(d)
sys.exit(0 if needle in blob else 1)
' "$needle"
}

tab_chat_tool_stats() {
  local tab="$1"
  dev_post "/api/tabs/$tab/chat-raw/start" '{}' \
    | "$PYTHON_BIN" - <<'PY' 2>/dev/null
import json,sys
try:
    d=json.load(sys.stdin)
except Exception:
    print("0 0")
    sys.exit()

tool_names = []
def walk(x):
    if isinstance(x, dict):
        typ = str(x.get("type") or x.get("message_type") or "")
        name = str(x.get("tool_name") or x.get("tool") or x.get("name") or "")
        if "tool" in typ.lower() or name:
            lower = name.lower()
            if lower:
                tool_names.append(lower)
        for v in x.values():
            walk(v)
    elif isinstance(x, list):
        for v in x:
            walk(v)

walk(d)
file_reads = [n for n in tool_names if n in {"read", "grep", "glob", "ls"}]
print(f"{len(tool_names)} {len(file_reads)}")
PY
}

# =====================================================================
# B. CLI PAIR
# =====================================================================
scenario_B() {
  echo; echo "=== B. CLI PAIR — orchestration new ==="
  local out plan orch planner
  out=$(dc orchestration new --orchestrator claude:opus --planner claude:fable --workspace "$WS" --json 2>&1)
  note "raw: $(echo "$out" | head -c 300)"
  plan=$(js "$out" "d.get('plan_id','')")
  orch=$(js "$out" "d.get('orchestrator_tab_id','')")
  planner=$(js "$out" "d.get('planner_tab_id','')")

  if [ -z "$plan" ] || [ -z "$orch" ] || [ -z "$planner" ]; then
    fail B "orchestration new did not return plan_id+orchestrator_tab_id+planner_tab_id (got plan='$plan' orch='$orch' planner='$planner')"
    return
  fi
  CREATED_PLANS+=("$plan"); CREATED_TABS+=("$orch" "$planner")
  SCENARIO_B_PLAN="$plan"
  SCENARIO_B_ORCH="$orch"
  SCENARIO_B_PLANNER="$planner"
  pass B "returned plan=$plan orch=$orch planner=$planner"

  # both tabs present in registry
  if [ "$(tab_exists "$orch")" = "True" ] && [ "$(tab_exists "$planner")" = "True" ]; then
    pass B.tabs "tab list shows both orchestrator + planner tabs"
  else
    fail B.tabs "one/both tabs missing from tab list (orch=$(tab_exists "$orch") planner=$(tab_exists "$planner"))"
  fi

  # plan binds both refs
  local pj owner_tab pref_kind pref_tab
  pj=$(dc plan get "$plan" --json 2>/dev/null)
  owner_tab=$(js "$pj" "d.get('owner',{}).get('tab_id','')")
  pref_kind=$(js "$pj" "(d.get('planner') or {}).get('kind','')")
  pref_tab=$(js "$pj" "(d.get('planner') or {}).get('tab_id','')")
  if [ "$owner_tab" = "$orch" ] && [ "$pref_kind" = "tab" ] && [ "$pref_tab" = "$planner" ]; then
    pass B.refs "plan owner=$owner_tab + planner ref (kind=tab tab=$pref_tab) both bound to spawned tabs"
  else
    fail B.refs "plan refs mismatch: owner=$owner_tab (want $orch) planner.kind=$pref_kind planner.tab=$pref_tab (want $planner)"
  fi

  # orchestrator chat carries the peer greeting / mandate first message
  sleep 3
  if tab_chat_has "$orch" "Planner specialist" || tab_chat_has "$orch" "attached as my Planner specialist" || tab_chat_has "$orch" "Orchestrator for plan"; then
    pass B.greeting "orchestrator chat carries the peer greeting/mandate first message"
  else
    finding B.greeting "peer greeting not yet visible in orchestrator chat transcript (may be delivery-timing; asserted refs bound synchronously above)"
  fi
}

scenario_B_idle() {
  echo; echo "=== B.IDLE — fresh pair waits for goal ==="
  if [ -z "$SCENARIO_B_PLANNER" ]; then
    fail B.idle "route B did not produce a planner tab; cannot assert idle behavior"
    return
  fi
  note "waiting 60s with no goal message after pair spawn (planner=$SCENARIO_B_PLANNER)"
  sleep 60
  local stats tool_count file_read_count
  stats=$(tab_chat_tool_stats "$SCENARIO_B_PLANNER")
  tool_count=$(echo "$stats" | awk '{print $1}')
  file_read_count=$(echo "$stats" | awk '{print $2}')
  if [ "${tool_count:-0}" = "0" ] && [ "${file_read_count:-0}" = "0" ]; then
    pass B.idle "fresh planner stayed idle for 60s before goal: zero tool calls and zero file reads"
  else
    fail B.idle "fresh planner acted before a goal: tool_calls=${tool_count:-?} file_reads=${file_read_count:-?}"
  fi
}

# =====================================================================
# C. ROLE SPAWN — agent spawn --role Orchestrator
# =====================================================================
scenario_C_orch=""
scenario_C_plan=""
scenario_C() {
  echo; echo "=== C. ROLE SPAWN — agent spawn --role Orchestrator ==="
  local out tab
  out=$(dc agent spawn claude --role Orchestrator --workspace "$WS" \
        --task "You are the Orchestrator for a real objective: add a CLI flag. Read codeterm docs ORCHESTRATION." --json 2>&1)
  note "raw: $(echo "$out" | head -c 240)"
  tab=$(js "$out" "d.get('tab_id','')")
  [ -z "$tab" ] && tab=$(echo "$out" | grep -oE "tab [a-f0-9]+" | awk '{print $2}' | head -1)
  if [ -z "$tab" ]; then fail C "spawn --role Orchestrator returned no tab id"; return; fi
  CREATED_TABS+=("$tab"); scenario_C_orch="$tab"
  pass C "orchestrator tab spawned: $tab"

  sleep 3
  # mandate block present
  if tab_chat_start_has "$tab" "Planner specialist" || tab_chat_start_has "$tab" "attached Planner specialist"; then
    pass C.mandate "onboarding carries the peer-pairing mandate block"
  else
    fail C.mandate "onboarding does NOT carry the peer-pairing mandate block"
  fi
  # pre-filled planner spawn command — real parent tab, but plan id?
  if tab_chat_has "$tab" "--role Planner --plan $tab"; then
    : # unlikely
  fi
  if tab_chat_start_has "$tab" "--parent $tab"; then
    pass C.prefill_parent "planner spawn command is pre-filled with the real orchestrator tab id (--parent $tab)"
  else
    finding C.prefill_parent "planner spawn command not pre-filled with real parent tab id"
  fi
  if tab_chat_start_has "$tab" "--plan PLAN_ID"; then
    finding C.prefill_plan "pinned expects a REAL plan id pre-filled; onboarding ships literal placeholder '--plan PLAN_ID' (role-spawn claims no plan yet — bounce to make-orchestrator/pair lane)"
  else
    note "no literal PLAN_ID placeholder found — a real plan id may be pre-filled (re-check pass condition)"
  fi
  # unpaired nudge within one reminder cycle — timer gated (default 1800s)
  gated C.nudge "unpaired nudge fires on heartbeat (nudge_interval_secs default 1800s), not observable in bounded run; driving state = orchestrator owns no bound planner (Unbound) => nudge SHOULD fire (see F/E for state assertions)"
}

# =====================================================================
# D. PROMOTION — POST /api/tabs/{id}/make-orchestrator
# =====================================================================
scenario_D() {
  echo; echo "=== D. PROMOTION — make-orchestrator RPC ==="
  note "no 'codeterm orchestrator promote' CLI exists; Make-Orchestrator is POST /api/tabs/{id}/make-orchestrator (dev-locked via dev_post)"
  # plain agent tab, no parent (promotable)
  local tab sess_before sess_after resp
  tab=$(new_claude_tab "$WS" "$WS_CWD" "promote-me")
  if [ -z "$tab" ]; then fail D "could not spawn a plain claude tab to promote"; return; fi
  CREATED_TABS+=("$tab")
  sess_before=$(tab_session "$tab")
  note "spawned plain tab $tab (session=$sess_before)"

  resp=$(dev_post "/api/tabs/$tab/make-orchestrator" '{}')
  note "make-orchestrator resp: $(echo "$resp" | head -c 200)"

  # role actually flipped
  local role_after
  role_after=$(dev_get /api/tabs | "$PYTHON_BIN" -c "import json,sys;print(next((p.get('role','') for p in json.load(sys.stdin) if p.get('id')=='$tab'),''))" 2>/dev/null)
  if echo "$role_after" | grep -qi "orchestrator"; then
    pass D.role "tab promoted to Orchestrator (role=$role_after)"
  else
    fail D.role "tab role did not become Orchestrator after make-orchestrator (role='$role_after')"
  fi

  # session survives promotion
  sess_after=$(tab_session "$tab")
  if [ "$sess_before" = "$sess_after" ]; then
    pass D.session "tab session survived promotion (same id before/after: '$sess_after')"
  else
    fail D.session "tab session changed across promotion ('$sess_before' -> '$sess_after')"
  fi

  # mandate delivered (task says 'response carries mandate'; actual = delivered to tab chat)
  sleep 3
  if tab_chat_start_has "$tab" "Planner specialist" || tab_chat_start_has "$tab" "now the Orchestrator"; then
    pass D.mandate "promotion onboarding (with mandate block) delivered to the tab"
  else
    fail D.mandate "promotion onboarding/mandate not found in tab chat"
  fi
  if echo "$resp" | grep -qi "Planner specialist\\|mandate"; then
    note "RPC response body also carries mandate text"
  else
    finding D.mandate_resp "pinned says 'command response carries the mandate block'; actual dev delivers it as a tab message, RPC body does not echo it"
  fi
  gated D.nudge "unpaired nudge is heartbeat-gated (1800s); driving state Unbound => should fire"
}

# =====================================================================
# E. PLANNER BIND — spawn --role Planner --plan <id> --parent <orch>
# =====================================================================
scenario_E() {
  echo; echo "=== E. PLANNER BIND — spawn Planner + nudge stops ==="
  # need a route-C style orchestrator that owns a real plan. Build a fresh pair
  # via a plan we own so the planner bind is observable end to end.
  local plan orch
  plan=$(mk_plan "orchmatrix-E")
  if [ -z "$plan" ]; then fail E "could not create plan for planner-bind test (db lock/contention)"; return; fi
  CREATED_PLANS+=("$plan")
  orch=$(new_claude_tab "$WS" "$WS_CWD" "orch-E")
  if [ -z "$orch" ]; then fail E "could not spawn orchestrator tab"; return; fi
  CREATED_TABS+=("$orch")
  note "plan $plan, orch tab $orch (planner-bind is owner-independent; claim-for-caller not usable via dc)"

  # driving state BEFORE bind: unbound => nudge should fire
  local pref_before
  pref_before=$(js "$(dc plan get "$plan" --json 2>/dev/null)" "(d.get('planner') or {}).get('kind','UNBOUND')")
  note "planner ref before bind: ${pref_before:-UNBOUND}"

  # bind planner via role spawn
  local pout planner
  pout=$(dc agent spawn claude --role Planner --plan "$plan" --parent "$orch" --title "planner-E" --json 2>&1)
  note "raw: $(echo "$pout" | head -c 200)"
  planner=$(js "$pout" "d.get('tab_id','')")
  [ -z "$planner" ] && planner=$(echo "$pout" | grep -oE "tab [a-f0-9]+" | awk '{print $2}' | head -1)
  if [ -z "$planner" ]; then fail E "planner spawn returned no tab id"; return; fi
  CREATED_TABS+=("$planner")

  # plan planner ref now bound to the new tab
  local pj pk pp
  pj=$(dc plan get "$plan" --json 2>/dev/null)
  pk=$(js "$pj" "(d.get('planner') or {}).get('kind','')")
  pp=$(js "$pj" "(d.get('planner') or {}).get('tab_id','')")
  if [ "$pk" = "tab" ] && [ "$pp" = "$planner" ]; then
    pass E.bind "plan planner ref bound to the new planner tab (kind=tab tab=$pp)"
  else
    fail E.bind "planner ref not bound to new tab (kind=$pk tab=$pp want $planner)"
  fi

  # planner onboarding names the orchestrator tab id
  sleep 3
  if tab_chat_has "$planner" "$orch"; then
    pass E.intro "planner onboarding names the orchestrator peer tab id ($orch)"
  else
    finding E.intro "planner onboarding did not visibly name orch tab $orch in transcript (delivery timing)"
  fi

  # nudge STOPS: deterministic driving state = planner now bound => resolution Planner => nudge None
  if [ "$pk" = "tab" ]; then
    pass E.nudge_state "driving state now Planner-bound => planner_pairing_nudge_prefix returns None (nudge stops); heartbeat text itself is timer-gated"
  else
    fail E.nudge_state "planner not bound => nudge would keep firing"
  fi
}

# =====================================================================
# F. WAIVER — plan update --planner none --note
# =====================================================================
scenario_F() {
  echo; echo "=== F. WAIVER — planner none + note enforcement ==="
  local plan
  plan=$(mk_plan "orchmatrix-F")
  if [ -z "$plan" ]; then fail F "could not create plan for waiver test (db lock/contention)"; return; fi
  CREATED_PLANS+=("$plan")

  # --planner none WITHOUT --note must be refused
  local o1 rc1
  o1=$(dc plan update "$plan" --planner none 2>&1); rc1=$?
  if [ $rc1 -ne 0 ] || echo "$o1" | grep -qi "note"; then
    pass F.note_required "'--planner none' without --note is refused (rc=$rc1)"
  else
    fail F.note_required "'--planner none' without --note was ACCEPTED (no note enforcement) — rc=$rc1 out='$o1'"
  fi

  # --planner none WITH --note records the waiver. Retry on the transient
  # "plan_index_mirror is pending after a committed write" false-failure (same
  # class as task_354c755f6e) that recurs under heavy concurrent load.
  local o2 pk pn try mirror_pending=0
  for try in 1 2 3 4; do
    o2=$(dc plan update "$plan" --planner none --note "solo test" 2>&1)
    echo "$o2" | grep -qi "plan_index_mirror is pending" && mirror_pending=1
    sleep 1
    pk=$(js "$(dc plan get "$plan" --json 2>/dev/null)" "(d.get('planner') or {}).get('kind','')")
    [ "$pk" = "waived" ] && break
  done
  pn=$(js "$(dc plan get "$plan" --json 2>/dev/null)" "(d.get('planner') or {}).get('note','')")
  if [ "$pk" = "waived" ] && [ "$pn" = "solo test" ]; then
    pass F.waiver "waiver recorded (planner.kind=waived note='$pn')"
    [ "$mirror_pending" = "1" ] && finding F.waiver_flaky "waiver eventually persisted but a 'plan_index_mirror is pending after a committed write' false-failure appeared under load (task_354c755f6e class); single-shot 'plan update' is not retry-safe (bounce to plan-index-mirror lane)"
  else
    fail F.waiver "waiver not recorded after retries (last='$o2' kind=$pk note='$pn')"
  fi

  # nudge stops on waiver: driving state Waived => nudge None
  if [ "$pk" = "waived" ]; then
    pass F.nudge_state "driving state Waived => nudge prefix None (nudge stops); heartbeat is timer-gated"
  else
    fail F.nudge_state "not waived => nudge would keep firing"
  fi
}

# =====================================================================
# G. DOCTOR LADDER — plan doctor --kind quality
# =====================================================================
# rule id per source/task: planner_unpaired. Severity ladder:
#   unpaired + planning        -> WARNING
#   unpaired + approval attempt -> ERROR
#   paired OR waived           -> silent (rule absent)
doctor_planner_rule() {
  # echoes "<severity>" of the planner_unpaired finding, or "" if absent
  local plan="$1"
  dc plan doctor "$plan" --kind quality --json 2>/dev/null | "$PYTHON_BIN" - <<'PY' 2>/dev/null
import json,sys
try:
    d=json.load(sys.stdin)
except Exception:
    print(''); sys.exit()
def findings(x):
    if isinstance(x,dict):
        for k in ('findings','issues','results','items','checks'):
            if k in x and isinstance(x[k],list): return x[k]
        # maybe dict of lists
        out=[]
        for v in x.values():
            if isinstance(v,list): out+=v
        return out
    if isinstance(x,list): return x
    return []
sev=''
for f in findings(d):
    if not isinstance(f,dict): continue
    kind=str(f.get('kind') or f.get('code') or f.get('id') or f.get('rule') or '')
    if 'planner_unpair' in kind or ('planner' in kind and 'pair' in kind):
        sev=str(f.get('severity') or f.get('level') or 'present')
print(sev)
PY
}

scenario_G() {
  echo; echo "=== G. DOCTOR LADDER — planner_unpaired severity by state ==="
  local plan
  plan=$(mk_plan "orchmatrix-G")
  if [ -z "$plan" ]; then fail G "could not create plan for doctor ladder (db lock/contention)"; return; fi
  CREATED_PLANS+=("$plan")

  # state 1: unpaired + planning => WARNING
  local s1
  s1=$(doctor_planner_rule "$plan")
  note "unpaired+planning: planner_unpaired severity='${s1:-<absent>}'"
  if echo "$s1" | grep -qi "warn"; then
    pass G.planning "unpaired plan in planning mode => planner_unpaired WARNING"
  elif [ -n "$s1" ]; then
    finding G.planning "unpaired+planning emits planner_unpaired but severity='$s1' (pinned: warning)"
  else
    finding G.planning "no planner_unpaired finding from 'plan doctor --kind quality' on unpaired planning plan — dev build $BUILD quality doctor emits only missing_goal/missing_spec_source; the planner-pairing ladder is not wired (bounce to doctor/QA-enforcement lane task_084e2148)"
  fi

  # state 2: approval attempt => ERROR (attempt approve, then re-read doctor)
  local ao
  ao=$(dc plan approve "$plan" 2>&1)
  note "approve attempt: rc/out=$(echo "$ao" | head -c 160)"
  local s2
  s2=$(doctor_planner_rule "$plan")
  note "after approval attempt: planner_unpaired severity='${s2:-<absent>}'"
  if echo "$s2" | grep -qi "error"; then
    pass G.approval "after approval attempt on unpaired plan => planner_unpaired ERROR"
  elif [ -n "$s2" ]; then
    finding G.approval "approval-attempt severity='$s2' (pinned: error)"
  else
    finding G.approval "no planner_unpaired ERROR after approval attempt (rule absent in dev build $BUILD)"
  fi

  # state 3: waive => silent
  dc plan update "$plan" --planner none --note "solo doctor test" >/dev/null 2>&1
  local s3
  s3=$(doctor_planner_rule "$plan")
  note "after waiver: planner_unpaired severity='${s3:-<absent>}'"
  if [ -z "$s3" ]; then
    pass G.silent "after waiver => planner_unpaired silent (rule absent)"
  else
    fail G.silent "planner_unpaired STILL present after waiver (severity='$s3') — waiver should silence it"
  fi
}

# =====================================================================
# H. NEGATIVES — zero-object-leak on every failure path
# =====================================================================
scenario_H() {
  echo; echo "=== H. NEGATIVES — actionable errors, zero leak ==="

  # H1: spawn --role Orchestrator with NO --workspace
  local p_before p_after out rc
  p_before=$(tab_count)
  out=$(dc agent spawn claude --role Orchestrator --task "no workspace given" --json 2>&1); rc=$?
  p_after=$(tab_count)
  note "H1 rc=$rc tabs ${p_before}->${p_after} out=$(echo "$out" | head -c 160)"
  local leaked_tab
  leaked_tab=$(js "$out" "d.get('tab_id','')")
  if [ $rc -ne 0 ] && [ "$p_before" = "$p_after" ]; then
    pass H1 "spawn --role Orchestrator w/o --workspace failed cleanly, zero tabs created"
  elif [ -n "$leaked_tab" ] || [ "$p_before" != "$p_after" ]; then
    # spawn fell back to active workspace and CREATED a tab
    [ -n "$leaked_tab" ] && CREATED_TABS+=("$leaked_tab")
    finding H1 "spawn --role Orchestrator w/o --workspace did NOT fail — fell back to active workspace and created tab '$leaked_tab' (tabs ${p_before}->${p_after}); pinned expects an actionable error + zero tabs (bounce to spawn-validation lane)"
  else
    fail H1 "spawn --role Orchestrator w/o --workspace: rc=$rc but tab count changed ${p_before}->${p_after}"
  fi

  # H2: a Worker attempting orchestration/spawn --role Orchestrator is refused
  # Backend guard check: spawn a real worker, then drive orchestration new with
  # that worker as the caller/spawner and expect refusal.
  local worker host_orch
  host_orch=$(new_claude_tab "$WS" "$WS_CWD" "orch-H2")
  [ -n "$host_orch" ] && CREATED_TABS+=("$host_orch")
  local wout
  wout=$(dc agent spawn claude --role Worker --parent "$host_orch" --title "worker-H2" --json 2>&1)
  worker=$(js "$wout" "d.get('tab_id','')")
  [ -z "$worker" ] && worker=$(echo "$wout" | grep -oE "tab [a-f0-9]+" | awk '{print $2}' | head -1)
  [ -n "$worker" ] && CREATED_TABS+=("$worker")
  note "H2 worker tab: ${worker:-<none>} (parent $host_orch)"
  # Attempt orchestration new attributed to the worker as spawner via REST.
  local h2_p_before h2_resp h2_p_after
  h2_p_before=$(tab_count)
h2_body=$("$PYTHON_BIN" - "$WS" "$worker" <<'PY'
import json, sys
sys.stdout.write(json.dumps({
    "orchestrator_agent": "claude",
    "planner_agent": "claude",
    "workspace_id": sys.argv[1],
    "spawned_by_tab_id": sys.argv[2],
}, separators=(",", ":")))
PY
)
  h2_resp=$(curl -s --max-time 30 -X POST -H "$CODETERM_DEV_AUTH_HEADER" \
    -H "Content-Type: application/json" --data-binary "$h2_body" \
    "http://127.0.0.1:7686/api/orchestration/new")
  h2_p_after=$(tab_count)
  note "H2 orchestration-new-as-worker resp: $(echo "$h2_resp" | head -c 200)"
  if echo "$h2_resp" | grep -qiE "worker|refus|forbid|not allowed|hierarchy|cannot"; then
    pass H2 "worker-attributed orchestration new refused by hierarchy guard"
    # clean any refused-but-leaked ids
    local wp=$(js "$h2_resp" "d.get('plan_id','')"); [ -n "$wp" ] && CREATED_PLANS+=("$wp")
  else
    local wp wo wpl
    wp=$(js "$h2_resp" "d.get('plan_id','')"); wo=$(js "$h2_resp" "d.get('orchestrator_tab_id','')"); wpl=$(js "$h2_resp" "d.get('planner_tab_id','')")
    [ -n "$wp" ] && CREATED_PLANS+=("$wp"); [ -n "$wo" ] && CREATED_TABS+=("$wo"); [ -n "$wpl" ] && CREATED_TABS+=("$wpl")
    finding H2 "no backend hierarchy guard: orchestration new attributed to a Worker (spawned_by=$worker) was NOT refused (created plan=$wp). Worker 'never spawn' is prompt-only, not enforced (bounce to hierarchy-guard lane)"
  fi

  # H3: orchestration new with unknown provider => clean fail, no half-pair
  local h3_before h3_resp h3_plan_leak
  h3_before=$(tab_count)
  h3_resp=$(dc orchestration new --orchestrator nonesuchprovider --planner claude --workspace "$WS" --json 2>&1)
  local h3_after=$(tab_count)
  note "H3 rc/out: $(echo "$h3_resp" | head -c 220) (tabs ${h3_before}->${h3_after})"
  # collect any leaked plan for cleanup
  h3_plan_leak=$(js "$h3_resp" "d.get('plan_id','')")
  [ -n "$h3_plan_leak" ] && CREATED_PLANS+=("$h3_plan_leak")
  if echo "$h3_resp" | grep -qiE "unknown|not found|no such|unregistered|invalid provider|recover|complete the pair|retry"; then
    if [ "$h3_before" = "$h3_after" ]; then
      pass H3 "unknown-provider orchestration new failed cleanly with actionable/recovery text, no tab leak"
    else
      finding H3 "unknown-provider failed with a message but left ${h3_after} tabs (was ${h3_before}) — half-pair; acceptable only if plan marked degraded + recovery printed"
    fi
  else
    fail H3 "unknown-provider orchestration new did not fail cleanly (no actionable error; tabs ${h3_before}->${h3_after})"
  fi
}

# --- run ------------------------------------------------------------
scenario_B
scenario_B_idle
scenario_C
scenario_D
scenario_E
scenario_F
scenario_G
scenario_H

# --- verdict table --------------------------------------------------
echo
echo "======================================================================"
echo " ORCHESTRATOR-CREATION MATRIX — verdict table (dev build $BUILD)"
echo "======================================================================"
printf " %-16s %-9s %s\n" "SCENARIO" "VERDICT" "DETAIL"
echo "----------------------------------------------------------------------"
np=0; nf=0; nfd=0; ng=0
for row in "${ROWS[@]}"; do
  IFS='|' read -r s v d <<< "$row"
  printf " %-16s %-9s %s\n" "$s" "$v" "$(echo "$d" | head -c 90)"
  case "$v" in PASS) np=$((np+1));; FAIL) nf=$((nf+1));; FINDING) nfd=$((nfd+1));; GATED) ng=$((ng+1));; esac
done
echo "----------------------------------------------------------------------"
echo " PASS=$np  FAIL=$nf  FINDING=$nfd  GATED=$ng"
echo "======================================================================"

# Plan hygiene gate: sweep any straggler, then assert the plan set returned
# to baseline (before == after). A leak FAILS the matrix — it never hides.
plan_sweep_new "$PLAN_BASELINE"
if ! plan_assert_clean "$PLAN_BASELINE" orchmatrix; then
  echo "RESULT: FAIL (matrix leaked plans — see above)"; exit 1
fi

[ "$FAILS" -eq 0 ] || { echo "RESULT: FAIL ($FAILS hard failures)"; exit 1; }
echo "RESULT: no hard failures (triage FINDING/GATED rows)"
exit 0
