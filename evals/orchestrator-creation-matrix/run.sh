#!/usr/bin/env bash
# WHAT: exercise orchestrator-creation routes B-H against the dev CLI and API.
# WHY: catch current CLI and pairing contract drift without mutating prod.
# ORACLE: each row checks CLI/error output and observable Tab, Plan, or chat state.
# Task: task_1800fa9977ea418094bb4ab291b2e3f2. All CLI calls use the `dc` wrapper.
# Route I (in-place picker UI) is covered by the Playwright suite, not this matrix.
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
source "$ROOT/evals/_lib/role_preset.sh"
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
SCENARIO_B_PLAN_COUNT=""
SCENARIO_B_OUTPUT=""
SCENARIO_C_OUTPUT=""

record() { ROWS+=("$1|$2|$3"); }
note()   { echo "    · $*"; }

pass()    { echo "  [PASS]    $1: $2"; record "$1" PASS "$2"; }
fail()    {
  echo "  [FAIL]    $1: $2"
  printf '        error body:\n%s\n' "${FAILURE_ERROR_BODY:-<empty>}"
  record "$1" FAIL "$2"
  FAILS=$((FAILS+1))
  FAILURE_ERROR_BODY=""
}
finding() { echo "  [FINDING] $1: $2"; record "$1" FINDING "$2"; }
gated()   { echo "  [GATED]   $1: $2"; record "$1" GATED "$2"; }

# --- json helpers (python; portable) --------------------------------
# js <output> <python-expr-on-d>; tolerate CLI hints around JSON.
js() {
  printf '%s' "$1" | "$PYTHON_BIN" -c '
import json,sys
raw=sys.stdin.read()
decoder=json.JSONDecoder()
for index,char in enumerate(raw):
    if char not in "[{":
        continue
    try:
        d,_=decoder.raw_decode(raw[index:])
        break
    except json.JSONDecodeError:
        pass
else:
    print("")
    raise SystemExit(0)
try:
    v=eval(sys.argv[1],{'d':d})
    print('' if v is None else v)
except Exception:
    print('')
' "$2" 2>/dev/null
}

print_error_body() {
  local label="$1" body="$2"
  if [ -n "$body" ]; then
    printf '    · %s error body:\n%s\n' "$label" "$body"
  else
    printf '    · %s error body: <empty>\n' "$label"
  fi
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
if ! dev_get /api/health >/dev/null 2>&1; then
  echo "ABORT: dev daemon not healthy on 7686. Start it (orchestrator owns dev): ./build.sh dev" >&2
  exit 7
fi
BUILD=$(dev_get /api/health | "$PYTHON_BIN" -c "import json,sys;print(json.load(sys.stdin).get('build','?'))" 2>/dev/null)
echo "  dev build: $BUILD"

# Creation routes must all reach the one shared preset resolver; a kind that
# escalated capability would poison every route below.
ROLE_PRESET_ERROR=""
if ROLE_PRESET_ERROR=$(assert_manager_kinds_resolve 2>&1) && \
   ROLE_PRESET_ERROR+="$(assert_planner_team_authority 2>&1)"; then
  pass "role-preset" "every creation-route kind resolves through the shared resolver"
else
  FAILURE_ERROR_BODY="$ROLE_PRESET_ERROR"
  fail "role-preset" "role kind capability or manager guidance drifted"
fi

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

# mk_plan <title> <orchestrator> -> sets PLAN_ID and PLAN_OUTPUT. It retries a
# bounded number of times on the plan store's transient database lock error.
PLAN_ID=""
PLAN_OUTPUT=""
mk_plan() {
  local title="$1" orchestrator="$2" out id try rc=0
  PLAN_ID=""
  PLAN_OUTPUT=""
  for try in 1 2 3 4 5; do
    out=$(dc --as-tab "$orchestrator" plan create --title "$title" --json 2>&1)
    rc=$?
    PLAN_OUTPUT="$out"
    id=$(js "$out" "d.get('plan_id') or d.get('plan',{}).get('id') or d.get('id','')")
    [ -n "$id" ] && { PLAN_ID="$id"; return 0; }
    echo "$out" | grep -qi "database is locked" || break
  done
  [ "$rc" -eq 0 ] && [ -z "$PLAN_OUTPUT" ] && PLAN_OUTPUT="plan create returned no JSON plan id"
  return 1
}

tab_rows_expr="(d if isinstance(d,list) else d.get('tabs',[]))"
tab_count() {
  local output
  output=$(dc tab list --json 2>&1)
  js "$output" "len($tab_rows_expr)"
}
plan_count() {
  local output
  output=$(dc plan list --json 2>&1)
  js "$output" "len(d if isinstance(d,list) else d.get('plans',[]))"
}
tab_exists() {
  local output
  output=$(dc tab list --json 2>&1)
  js "$output" "any(row.get('id') == '$1' for row in $tab_rows_expr)"
}
tab_session() { dev_get "/api/tabs" | "$PYTHON_BIN" -c "import json,sys;print(next((p.get('session_id') or p.get('session') or '' for p in json.load(sys.stdin) if p.get('id')=='$1'),''))" 2>/dev/null; }

# grep the UI chat-start snapshot for a fixed string. This is the deterministic
# oracle for pending_greeting; `tab chat` only reads the durable transcript.
chat_raw_stop() {
  local tab="$1" subscription="$2" body output rc
  body=$(printf '{"subscriptionId":"%s"}' "$subscription")
  output=$(dev_post "/api/tabs/$tab/chat-raw/stop" "$body" 2>&1)
  rc=$?
  if [ "$rc" -ne 0 ]; then
    note "chat-raw stop failed for $tab: $output"
  fi
  return "$rc"
}

run_tab_event_wait() {
  local mode="$1" tab="$2" timeout_seconds="$3" subscription="$4" api auth output rc previous_pipefail=0 needles
  shift 4
  api=$(dev_api_url) || { FAILURE_ERROR_BODY="could not resolve the dev API URL"; return 1; }
  auth=$(dev_auth_header) || { FAILURE_ERROR_BODY="could not resolve the dev API auth header"; return 1; }
  needles=$(printf '%s\n' "$@" | "$PYTHON_BIN" -c 'import json,sys; print(json.dumps(sys.stdin.read().splitlines()))')
  shopt -qo pipefail && previous_pipefail=1
  set -o pipefail
  output=$(timeout "$timeout_seconds" curl -N --silent --show-error --include \
    -H "$auth" "$api/api/events/stream?tab_id=$tab" \
    | env WAIT_API_URL="$api" WAIT_AUTH_HEADER="$auth" WAIT_TAB_ID="$tab" \
      WAIT_MODE="$mode" WAIT_SUBSCRIPTION_ID="$subscription" \
      WAIT_SECONDS="$timeout_seconds" WAIT_NEEDLES="$needles" \
      "$PYTHON_BIN" "$ROOT/evals/orchestrator-creation-matrix/wait_tab_event.py" 2>&1)
  rc=$?
  [ "$previous_pipefail" -eq 1 ] || set +o pipefail
  if echo "$output" | grep -q '^MATCH:'; then
    note "tab event satisfied $mode for $tab: $(echo "$output" | grep '^MATCH:' | tail -1)"
    return 0
  fi
  if [ "$mode" = "idle" ] && [ "$rc" -eq 124 ] \
     && echo "$output" | grep -q '^QUIET_WINDOW_COMPLETE$'; then
    return 0
  fi
  FAILURE_ERROR_BODY="$output (wait exit=$rc)"
  return 1
}

wait_for_chat_start_has() {
  local tab="$1" timeout_seconds="$2" subscription rc
  shift 2
  subscription="orchmatrix-$$-${RANDOM}-${tab}"
  run_tab_event_wait chat_start "$tab" "$timeout_seconds" "$subscription" "$@"
  rc=$?
  chat_raw_stop "$tab" "$subscription" || true
  return "$rc"
}

wait_for_chat_start_all() {
  local tab="$1" timeout_seconds="$2" subscription rc
  shift 2
  subscription="orchmatrix-$$-${RANDOM}-${tab}"
  run_tab_event_wait chat_start_all "$tab" "$timeout_seconds" "$subscription" "$@"
  rc=$?
  chat_raw_stop "$tab" "$subscription" || true
  return "$rc"
}

wait_for_chat_transcript_has() {
  local tab="$1" timeout_seconds="$2"
  shift 2
  run_tab_event_wait transcript "$tab" "$timeout_seconds" "" "$@"
}

SPAWN_TAB=""
SPAWN_OUTPUT=""
SPAWN_RC=0
spawn_plain_claude() {
  local title="$1"
  SPAWN_OUTPUT=$(dc agent spawn claude --workspace "$WS" --cwd "$WS_CWD" \
    --title "$title" --json 2>&1)
  SPAWN_RC=$?
  SPAWN_TAB=$(js "$SPAWN_OUTPUT" "d.get('tab_id','')")
  [ -n "$SPAWN_TAB" ] || SPAWN_TAB=$(echo "$SPAWN_OUTPUT" | grep -oE "tab [a-f0-9]+" | awk '{print $2}' | head -1)
  [ -n "$SPAWN_TAB" ]
}

# =====================================================================
# B. CLI PAIR
# =====================================================================
scenario_B() {
  echo; echo "=== B. CLI PAIR — team pair ==="
  local out orch planner plan_field plan_before plan_after rc
  plan_before=$(plan_count)
  out=$(dc team pair --orchestrator claude:opus --planner claude:fable --workspace "$WS" --json 2>&1)
  rc=$?
  note "team pair output: $out"
  SCENARIO_B_OUTPUT="$out"
  orch=$(js "$out" "d.get('orchestrator_tab_id','')")
  planner=$(js "$out" "d.get('planner_tab_id','')")

  if [ "$rc" -ne 0 ] || [ -z "$orch" ] || [ -z "$planner" ]; then
    FAILURE_ERROR_BODY="$out"
    fail B "team pair did not return orchestrator_tab_id+planner_tab_id (got orch='$orch' planner='$planner')"
    return
  fi
  CREATED_TABS+=("$orch" "$planner")
  SCENARIO_B_ORCH="$orch"
  SCENARIO_B_PLANNER="$planner"
  plan_field=$(js "$out" "'plan_id' in d")
  plan_after=$(plan_count)
  SCENARIO_B_PLAN_COUNT="$plan_after"
  if [ "$plan_field" = "False" ] && [ -n "$plan_before" ] && \
     [ "$plan_before" = "$plan_after" ]; then
    pass B "returned orchestrator=$orch and planner=$planner; response has no plan_id and pair created no Plan before a first goal"
  else
    FAILURE_ERROR_BODY="$out
plan counts before/after: ${plan_before:-<unavailable>}/${plan_after:-<unavailable>}
plan list: $(dc plan list --json 2>&1)"
    fail B "pair response plan_id field='$plan_field'; plan count ${plan_before:-<unavailable>}->${plan_after:-<unavailable>} before any goal"
  fi

  # both tabs present in registry
  if [ "$(tab_exists "$orch")" = "True" ] && [ "$(tab_exists "$planner")" = "True" ]; then
    pass B.tabs "tab list shows both orchestrator + planner tabs"
  else
    FAILURE_ERROR_BODY=$(dev_get /api/tabs 2>&1)
    fail B.tabs "one/both tabs missing from tab list (orch=$(tab_exists "$orch") planner=$(tab_exists "$planner"))"
  fi

  # orchestrator chat carries the peer greeting / mandate first message
  if wait_for_chat_transcript_has "$orch" 30 "Planner specialist" "attached as my Planner specialist" "Orchestrator for plan"; then
    pass B.greeting "orchestrator chat carries the peer greeting/mandate first message"
  else
    FAILURE_ERROR_BODY=$(dc tab chat --tab "$orch" --json 2>&1)
    fail B.greeting "orchestrator chat did not show the peer greeting within 30s"
  fi
}

scenario_B_idle() {
  echo; echo "=== B.IDLE — fresh pair waits for goal ==="
  if [ -z "$SCENARIO_B_PLANNER" ]; then
    FAILURE_ERROR_BODY="$SCENARIO_B_OUTPUT"
    fail B.idle "route B did not produce a planner tab; cannot assert idle behavior"
    return
  fi
  note "observing Planner idleness on one tab-filtered event stream for 60s (planner=$SCENARIO_B_PLANNER)"
  local idle_subscription="orchmatrix-$$-${RANDOM}-${SCENARIO_B_PLANNER}" plan_count_now
  if ! run_tab_event_wait idle "$SCENARIO_B_PLANNER" 60 "$idle_subscription"; then
    chat_raw_stop "$SCENARIO_B_PLANNER" "$idle_subscription" || true
    fail B.idle "Planner acted before a goal or the bounded event observation failed"
    return
  fi
  chat_raw_stop "$SCENARIO_B_PLANNER" "$idle_subscription" || true
  plan_count_now=$(plan_count)
  if [ -n "$plan_count_now" ] && [ "$plan_count_now" = "$SCENARIO_B_PLAN_COUNT" ] && [ -z "$SCENARIO_B_PLAN" ]; then
    pass B.idle "fresh Planner stayed idle for 60s and no Plan existed before the first goal"
  else
    FAILURE_ERROR_BODY=$(dc plan list --json 2>&1)
    fail B.idle "pre-goal plan count changed ${SCENARIO_B_PLAN_COUNT}->${plan_count_now} or a plan id was already bound"
    return
  fi

  mk_plan "orchmatrix-B-first-goal" "$SCENARIO_B_ORCH"
  if [ -z "$PLAN_ID" ]; then
    FAILURE_ERROR_BODY="$PLAN_OUTPUT"
    fail B.first_goal "first goal/plan creation returned no plan id"
    return
  fi
  plan="$PLAN_ID"
  CREATED_PLANS+=("$plan")
  SCENARIO_B_PLAN="$plan"
  pass B.first_goal "first plan creation returned $plan after pair response had no plan_id"

  local pj owner_tab pref_kind pref_tab
  pj=$(dc --as-tab "$SCENARIO_B_ORCH" plan get "$plan" --json 2>&1)
  owner_tab=$(js "$pj" "d.get('owner',{}).get('tab_id','')")
  pref_kind=$(js "$pj" "(d.get('planner') or {}).get('kind','')")
  pref_tab=$(js "$pj" "(d.get('planner') or {}).get('tab_id','')")
  if [ "$owner_tab" = "$SCENARIO_B_ORCH" ] && [ "$pref_kind" = "tab" ] && [ "$pref_tab" = "$SCENARIO_B_PLANNER" ]; then
    pass B.refs "first Plan owner=$owner_tab and planner ref (kind=tab tab=$pref_tab) bind to the pair tabs"
  else
    FAILURE_ERROR_BODY="$pj"
    fail B.refs "first Plan refs mismatch: owner=$owner_tab (want $SCENARIO_B_ORCH) planner.kind=$pref_kind planner.tab=$pref_tab (want $SCENARIO_B_PLANNER)"
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
  SCENARIO_C_OUTPUT="$out"
  tab=$(js "$out" "d.get('tab_id','')")
  [ -z "$tab" ] && tab=$(echo "$out" | grep -oE "tab [a-f0-9]+" | awk '{print $2}' | head -1)
  if [ -z "$tab" ]; then
    FAILURE_ERROR_BODY="$out"
    fail C "spawn --role Orchestrator returned no tab id"
    return
  fi
  CREATED_TABS+=("$tab"); scenario_C_orch="$tab"
  pass C "orchestrator tab spawned: $tab"

  # mandate block present
  if wait_for_chat_start_has "$tab" 30 "Planner specialist" "attached Planner specialist"; then
    pass C.mandate "onboarding carries the peer-pairing mandate block"
  else
    fail C.mandate "onboarding did not carry the peer-pairing mandate block within 30s"
  fi
  # One subscription and one event wait cover both pre-filled command fragments.
  if wait_for_chat_start_all "$tab" 30 "--parent $tab" "--plan PLAN_ID"; then
    pass C.prefill_parent "planner spawn command is pre-filled with the real orchestrator tab id (--parent $tab)"
    pass C.prefill_plan "onboarding leaves the Plan unbound until the first goal and uses a placeholder"
  else
    local prefill_error="$FAILURE_ERROR_BODY"
    fail C.prefill_parent "planner spawn command was not pre-filled with real parent tab id"
    FAILURE_ERROR_BODY="$prefill_error"
    fail C.prefill_plan "onboarding did not show the unbound-plan placeholder"
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
  local tab sess_before sess_after resp rpc_rc
  if ! spawn_plain_claude "promote-me"; then
    FAILURE_ERROR_BODY="$SPAWN_OUTPUT"
    fail D "could not spawn a plain claude tab to promote (rc=$SPAWN_RC)"
    return
  fi
  tab="$SPAWN_TAB"
  CREATED_TABS+=("$tab")
  sess_before=$(tab_session "$tab")
  note "spawned plain tab $tab (session=$sess_before)"

  resp=$(dev_post "/api/tabs/$tab/make-orchestrator" '{}' 2>&1)
  rpc_rc=$?
  note "make-orchestrator response: $resp"

  # role actually flipped
  local role_after
  role_after=$(dev_get /api/tabs | "$PYTHON_BIN" -c "import json,sys;print(next((p.get('role','') for p in json.load(sys.stdin) if p.get('id')=='$tab'),''))" 2>/dev/null)
  if echo "$role_after" | grep -qi "orchestrator"; then
    pass D.role "tab promoted to Orchestrator (role=$role_after)"
  else
    FAILURE_ERROR_BODY="$resp"
    fail D.role "tab role did not become Orchestrator after make-orchestrator (role='$role_after')"
  fi

  # session survives promotion
  sess_after=$(tab_session "$tab")
  if [ "$sess_before" = "$sess_after" ]; then
    pass D.session "tab session survived promotion (same id before/after: '$sess_after')"
  else
    FAILURE_ERROR_BODY=$(dev_get /api/tabs 2>&1)
    fail D.session "tab session changed across promotion ('$sess_before' -> '$sess_after')"
  fi

  # mandate delivered (task says 'response carries mandate'; actual = delivered to tab chat)
  if wait_for_chat_start_has "$tab" 30 "Planner specialist" "now the Orchestrator"; then
    pass D.mandate "promotion onboarding (with mandate block) delivered to the tab"
  else
    fail D.mandate "promotion onboarding/mandate not found in chat-raw within 30s"
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
  local plan orch
  orch="$scenario_C_orch"
  if [ -z "$orch" ]; then
    FAILURE_ERROR_BODY="$SCENARIO_C_OUTPUT"
    fail E "route C did not provide a live Orchestrator for plan authority"
    return
  fi
  mk_plan "orchmatrix-E" "$orch"
  if [ -z "$PLAN_ID" ]; then
    FAILURE_ERROR_BODY="$PLAN_OUTPUT"
    fail E "could not create the planner-bind Plan as its Orchestrator"
    return
  fi
  plan="$PLAN_ID"
  CREATED_PLANS+=("$plan")
  note "plan $plan created as Orchestrator $orch"

  # driving state BEFORE bind: unbound => nudge should fire
  local pref_before
  pref_before=$(js "$(dc --as-tab "$orch" plan get "$plan" --json 2>&1)" "(d.get('planner') or {}).get('kind','UNBOUND')")
  note "planner ref before bind: ${pref_before:-UNBOUND}"

  # bind planner via role spawn
  local pout planner
  pout=$(dc --as-tab "$orch" agent spawn claude --role Planner --plan "$plan" --parent "$orch" --title "planner-E" --json 2>&1)
  planner=$(js "$pout" "d.get('tab_id','')")
  [ -z "$planner" ] && planner=$(echo "$pout" | grep -oE "tab [a-f0-9]+" | awk '{print $2}' | head -1)
  if [ -z "$planner" ]; then
    FAILURE_ERROR_BODY="$pout"
    fail E "planner spawn returned no tab id"
    return
  fi
  CREATED_TABS+=("$planner")

  # plan planner ref now bound to the new tab
  local pj pk pp
  pj=$(dc --as-tab "$orch" plan get "$plan" --json 2>&1)
  pk=$(js "$pj" "(d.get('planner') or {}).get('kind','')")
  pp=$(js "$pj" "(d.get('planner') or {}).get('tab_id','')")
  if [ "$pk" = "tab" ] && [ "$pp" = "$planner" ]; then
    pass E.bind "plan planner ref bound to the new planner tab (kind=tab tab=$pp)"
  else
    FAILURE_ERROR_BODY="$pj"
    fail E.bind "planner ref not bound to new tab (kind=$pk tab=$pp want $planner)"
  fi

  # planner onboarding names the orchestrator tab id
  if wait_for_chat_transcript_has "$planner" 30 "$orch"; then
    pass E.intro "planner onboarding names the orchestrator peer tab id ($orch)"
  else
    FAILURE_ERROR_BODY=$(dc tab chat --tab "$planner" --json 2>&1)
    fail E.intro "planner onboarding did not name Orchestrator tab $orch within 30s"
  fi

  # nudge STOPS: deterministic driving state = planner now bound => resolution Planner => nudge None
  if [ "$pk" = "tab" ]; then
    pass E.nudge_state "driving state now Planner-bound => planner_pairing_nudge_prefix returns None (nudge stops); heartbeat text itself is timer-gated"
  else
    FAILURE_ERROR_BODY="$pj"
    fail E.nudge_state "planner not bound => nudge would keep firing"
  fi
}

# =====================================================================
# F. WAIVER — plan update --planner none --note
# =====================================================================
scenario_F() {
  echo; echo "=== F. WAIVER — planner none + note enforcement ==="
  local plan orch
  orch="$scenario_C_orch"
  if [ -z "$orch" ]; then FAILURE_ERROR_BODY="$SCENARIO_C_OUTPUT"; fail F "route C did not provide an Orchestrator for plan authority"; return; fi
  mk_plan "orchmatrix-F" "$orch"
  if [ -z "$PLAN_ID" ]; then
    FAILURE_ERROR_BODY="$PLAN_OUTPUT"
    fail F "could not create the waiver Plan as its Orchestrator"
    return
  fi
  plan="$PLAN_ID"
  CREATED_PLANS+=("$plan")

  # --planner none WITHOUT --note must be refused
  local o1 rc1
  o1=$(dc --as-tab "$orch" plan update "$plan" --planner none 2>&1); rc1=$?
  note "waiver without note response: $o1"
  if [ $rc1 -ne 0 ] || echo "$o1" | grep -qi "note"; then
    pass F.note_required "'--planner none' without --note is refused (rc=$rc1)"
  else
    FAILURE_ERROR_BODY="$o1"
    fail F.note_required "'--planner none' without --note was ACCEPTED (no note enforcement) — rc=$rc1 out='$o1'"
  fi

  # --planner none WITH --note records the waiver. Retry on the transient
  # "plan_index_mirror is pending after a committed write" false-failure (same
  # class as task_354c755f6e) that recurs under heavy concurrent load.
  local o2 pk pn try mirror_pending=0 waiver_rc waiver_plan
  for try in 1 2 3 4; do
    o2=$(dc --as-tab "$orch" plan update "$plan" --planner none --note "solo test" 2>&1)
    waiver_rc=$?
    echo "$o2" | grep -qi "plan_index_mirror is pending" && mirror_pending=1
    waiver_plan=$(dc --as-tab "$orch" plan get "$plan" --json 2>&1)
    pk=$(js "$waiver_plan" "(d.get('planner') or {}).get('kind','')")
    [ "$pk" = "waived" ] && break
  done
  pn=$(js "$waiver_plan" "(d.get('planner') or {}).get('note','')")
  note "waiver update response (rc=$waiver_rc): $o2"
  if [ "$pk" = "waived" ] && [ "$pn" = "solo test" ]; then
    pass F.waiver "waiver recorded (planner.kind=waived note='$pn')"
    [ "$mirror_pending" = "1" ] && finding F.waiver_flaky "waiver eventually persisted but a 'plan_index_mirror is pending after a committed write' false-failure appeared under load (task_354c755f6e class); single-shot 'plan update' is not retry-safe (bounce to plan-index-mirror lane)"
  else
    FAILURE_ERROR_BODY="$o2"
    fail F.waiver "waiver not recorded after retries (last='$o2' kind=$pk note='$pn')"
  fi

  # nudge stops on waiver: driving state Waived => nudge None
  if [ "$pk" = "waived" ]; then
    pass F.nudge_state "driving state Waived => nudge prefix None (nudge stops); heartbeat is timer-gated"
  else
    FAILURE_ERROR_BODY="$waiver_plan"
    fail F.nudge_state "not waived => nudge would keep firing"
  fi
}

# =====================================================================
# G. DOCTOR LADDER — plan doctor --kind quality
# =====================================================================
# doctor reports a warning; plan approve temporarily escalates that finding to
# an error while evaluating the approval gate. Waiver removes the finding.
DOCTOR_OUTPUT=""
DOCTOR_SEVERITY=""
DOCTOR_RC=0
doctor_planner_rule() {
  local orchestrator="$1" plan="$2"
  DOCTOR_OUTPUT=$(dc --as-tab "$orchestrator" plan doctor "$plan" --kind quality --json 2>&1)
  DOCTOR_RC=$?
  DOCTOR_SEVERITY=$(printf '%s' "$DOCTOR_OUTPUT" | "$PYTHON_BIN" -c '
import json,sys
raw=sys.stdin.read()
decoder=json.JSONDecoder()
for i,char in enumerate(raw):
    if char != "{": continue
    try:
        data,_=decoder.raw_decode(raw[i:])
        break
    except json.JSONDecodeError:
        pass
else:
    print("")
    raise SystemExit(0)
for finding in data.get("findings", []):
    if isinstance(finding, dict) and finding.get("kind") == "planner_unpaired":
        print(finding.get("severity", "present"))
        break
' 2>/dev/null)
}

scenario_G() {
  echo; echo "=== G. DOCTOR LADDER — planner_unpaired severity by state ==="
  local plan orch
  orch="$scenario_C_orch"
  if [ -z "$orch" ]; then FAILURE_ERROR_BODY="$SCENARIO_C_OUTPUT"; fail G "route C did not provide an Orchestrator for plan authority"; return; fi
  mk_plan "orchmatrix-G" "$orch"
  if [ -z "$PLAN_ID" ]; then
    FAILURE_ERROR_BODY="$PLAN_OUTPUT"
    fail G "could not create the doctor Plan as its Orchestrator"
    return
  fi
  plan="$PLAN_ID"
  CREATED_PLANS+=("$plan")

  # state 1: unpaired + planning => WARNING
  doctor_planner_rule "$orch" "$plan"
  local s1="$DOCTOR_SEVERITY"
  note "unpaired+planning: planner_unpaired severity='${s1:-<absent>}' (doctor rc=$DOCTOR_RC)"
  if [ "$s1" = "warning" ]; then
    pass G.planning "unpaired plan in planning mode => planner_unpaired WARNING"
  else
    FAILURE_ERROR_BODY="$DOCTOR_OUTPUT"
    fail G.planning "quality doctor returned planner_unpaired severity='${s1:-<absent>}' (want warning)"
  fi

  # Approval checks render the escalated finding in their failure body.
  local ao approve_rc
  ao=$(dc --as-tab "$orch" plan approve "$plan" 2>&1)
  approve_rc=$?
  note "approval response (rc=$approve_rc): $ao"
  if echo "$ao" | grep -qiE 'error planner_unpaired'; then
    pass G.approval "approval attempt renders planner_unpaired as an ERROR"
  else
    FAILURE_ERROR_BODY="$ao"
    fail G.approval "approval response did not render planner_unpaired as an ERROR"
  fi

  # state 3: waive => silent
  local waiver_output waiver_rc
  waiver_output=$(dc --as-tab "$orch" plan update "$plan" --planner none --note "solo doctor test" 2>&1)
  waiver_rc=$?
  note "doctor waiver response (rc=$waiver_rc): $waiver_output"
  doctor_planner_rule "$orch" "$plan"
  local s3
  s3="$DOCTOR_SEVERITY"
  note "after waiver: planner_unpaired severity='${s3:-<absent>}'"
  if [ "$waiver_rc" -eq 0 ] && [ -z "$s3" ]; then
    pass G.silent "after waiver => planner_unpaired is silent"
  else
    FAILURE_ERROR_BODY="$waiver_output${DOCTOR_OUTPUT:+\n$DOCTOR_OUTPUT}"
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
  if [ $rc -ne 0 ] && [ -n "$p_before" ] && [ "$p_before" = "$p_after" ]; then
    print_error_body "H1 spawn" "$out"
    pass H1 "spawn --role Orchestrator w/o --workspace failed cleanly, zero tabs created"
  elif [ -n "$leaked_tab" ] || [ "$p_before" != "$p_after" ]; then
    # spawn fell back to active workspace and CREATED a tab
    [ -n "$leaked_tab" ] && CREATED_TABS+=("$leaked_tab")
    finding H1 "spawn --role Orchestrator w/o --workspace did NOT fail — fell back to active workspace and created tab '$leaked_tab' (tabs ${p_before}->${p_after}); pinned expects an actionable error + zero tabs (bounce to spawn-validation lane)"
  else
    FAILURE_ERROR_BODY="$out"
    fail H1 "spawn --role Orchestrator w/o --workspace: rc=$rc but tab count changed ${p_before}->${p_after}"
  fi

  # H2: a Worker-attributed pair is refused without the leaf acknowledgement.
  local worker host_orch wout
  host_orch="$scenario_C_orch"
  if [ -z "$host_orch" ]; then
    FAILURE_ERROR_BODY="$SCENARIO_C_OUTPUT"
    fail H2 "route C did not provide an Orchestrator parent for the Worker fixture"
    return
  fi
  wout=$(dc --as-tab "$host_orch" agent spawn claude --role Worker --parent "$host_orch" \
    --workspace "$WS" --title "worker-H2" --json 2>&1)
  worker=$(js "$wout" "d.get('tab_id','')")
  [ -z "$worker" ] && worker=$(echo "$wout" | grep -oE "tab [a-f0-9]+" | awk '{print $2}' | head -1)
  if [ -z "$worker" ]; then
    FAILURE_ERROR_BODY="$wout"
    fail H2 "could not spawn the Worker fixture"
    return
  fi
  CREATED_TABS+=("$worker")
  note "H2 worker tab: ${worker:-<none>} (parent $host_orch)"
  # --as-tab makes the real CLI carry this Worker as spawned_by_tab_id.
  local h2_p_before h2_resp h2_p_after h2_rc h2_plan_before h2_plan_after
  h2_p_before=$(tab_count)
  h2_plan_before=$(plan_count)
  h2_resp=$(dc --as-tab "$worker" team pair --orchestrator claude --planner claude \
    --workspace "$WS" --json 2>&1)
  h2_rc=$?
  h2_p_after=$(tab_count)
  h2_plan_after=$(plan_count)
  print_error_body "H2 team pair" "$h2_resp"
  if [ "$h2_rc" -ne 0 ] && echo "$h2_resp" | grep -q -- "--acknowledge-leaf" \
      && [ -n "$h2_p_before" ] && [ -n "$h2_p_after" ] \
      && [ -n "$h2_plan_before" ] && [ -n "$h2_plan_after" ] \
      && [ "$h2_p_before" = "$h2_p_after" ] && [ "$h2_plan_before" = "$h2_plan_after" ]; then
    pass H2 "Worker-attributed team pair without --acknowledge-leaf was refused with no Plan or Tab created"
  else
    FAILURE_ERROR_BODY="$h2_resp
tab list: $(dc tab list --json 2>&1)
plan list: $(dc plan list --json 2>&1)"
    local leaked_pair_orch leaked_pair_planner
    leaked_pair_orch=$(js "$h2_resp" "d.get('orchestrator_tab_id','')")
    leaked_pair_planner=$(js "$h2_resp" "d.get('planner_tab_id','')")
    [ -n "$leaked_pair_orch" ] && CREATED_TABS+=("$leaked_pair_orch")
    [ -n "$leaked_pair_planner" ] && CREATED_TABS+=("$leaked_pair_planner")
    fail H2 "Worker pair was not refused cleanly (rc=$h2_rc, tabs ${h2_p_before}->${h2_p_after}, plans ${h2_plan_before}->${h2_plan_after})"
  fi

  # H3: an unknown Planner provider is rejected before the first pair Tab.
  local h3_before h3_resp h3_plan_before h3_plan_after h3_rc h3_orch_leak h3_planner_leak
  h3_before=$(tab_count)
  h3_plan_before=$(plan_count)
  h3_resp=$(dc team pair --orchestrator claude --planner nonesuchprovider --workspace "$WS" --json 2>&1)
  h3_rc=$?
  local h3_after=$(tab_count)
  h3_plan_after=$(plan_count)
  print_error_body "H3 team pair" "$h3_resp"
  h3_orch_leak=$(js "$h3_resp" "d.get('orchestrator_tab_id','')")
  h3_planner_leak=$(js "$h3_resp" "d.get('planner_tab_id','')")
  [ -n "$h3_orch_leak" ] && CREATED_TABS+=("$h3_orch_leak")
  [ -n "$h3_planner_leak" ] && CREATED_TABS+=("$h3_planner_leak")
  if [ "$h3_rc" -ne 0 ] && echo "$h3_resp" | grep -qiE "unknown (agent )?provider|unregistered provider|provider.*not found" \
      && [ -n "$h3_before" ] && [ -n "$h3_after" ] \
      && [ -n "$h3_plan_before" ] && [ -n "$h3_plan_after" ] \
      && [ "$h3_before" = "$h3_after" ] && [ "$h3_plan_before" = "$h3_plan_after" ]; then
    pass H3 "unknown-provider pair refused before creating a Plan or Tab"
  else
    FAILURE_ERROR_BODY="$h3_resp
tab list: $(dc tab list --json 2>&1)
plan list: $(dc plan list --json 2>&1)"
    fail H3 "unknown-provider pair did not leave zero residue (rc=$h3_rc, tabs ${h3_before}->${h3_after}, plans ${h3_plan_before}->${h3_plan_after})"
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
