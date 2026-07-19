#!/usr/bin/env bash
# Codex + Opencode session-restore drill. Spawns SCALE copies of each
# scenario (silent codex, codex with text, silent opencode, opencode with
# text) → total 4×SCALE panes. Kills dev, restarts, asserts every pane
# survived (no [Process exited]), session_id preserved, marker present
# for with-text panes. Plus one input-parser pane that types `codex` by
# hand. Env: SCALE=N (default 1).

set -u
source "$(git rev-parse --show-toplevel)/evals/_lib/lib.sh"
dev_env || exit 1
dev_alive || { echo "ERROR: dev not running. Start it: ./build.sh dev" >&2; exit 1; }

SCALE="${SCALE:-1}"
CWD_BASE="$(pwd)/target-check/evals/codex-opencode-restore-cwd"
mkdir -p "$CWD_BASE"

echo "== reset dev HARD"
reset_dev_state_hard

# Scenario table: <kind>|<marker>|<task>
# kind=cx-s / cx-t / oc-s / oc-t; marker empty for silent panes.
SCENARIOS=(
  "cx-s|codex|||"
  "cx-t|codex|ANANASMARKER|reply with single word ANANASMARKER"
  "oc-s|opencode|||"
  "oc-t|opencode|PIRAMIDAMARKER|reply with single word PIRAMIDAMARKER"
)

declare -a PANE_IDS PANE_LABELS PANE_MARKERS

echo "== spawn ${SCALE}× of each scenario (4×$SCALE = $((4*SCALE)) panes)"
for entry in "${SCENARIOS[@]}"; do
  IFS='|' read -r kind provider marker task <<<"$entry"
  for i in $(seq 1 "$SCALE"); do
    suffix=$([ "$SCALE" -gt 1 ] && echo "-$i" || echo "")
    ws_name="WS-${kind}${suffix}"
    cwd="$CWD_BASE/${kind}${suffix}"
    title="${kind}${suffix}"
    ws_id=$(new_workspace "$ws_name" "$cwd")
    if [ "$provider" = "codex" ]; then
      pid=$(new_codex_pane "$ws_id" "$cwd" "$title" "$task")
    else
      pid=$(new_opencode_pane "$ws_id" "$cwd" "$title" "$task")
    fi
    PANE_IDS+=("$pid")
    PANE_LABELS+=("$title")
    PANE_MARKERS+=("$marker")
    echo "  spawned $title pane=$pid ws=$ws_id"
  done
done

echo "== wait 25s for TUI bootstrap + first model response"
sleep 25

echo "== assert PRE-kill state"
for i in "${!PANE_IDS[@]}"; do
  pid="${PANE_IDS[$i]}"
  label="${PANE_LABELS[$i]}"
  marker="${PANE_MARKERS[$i]}"
  out=$(dc tab get-text --tab "$pid" 2>&1 || echo "")
  if [ -z "$marker" ] || echo "$out" | grep -q "$marker"; then
    echo "  OK pre-$label"
  else
    echo "  FAIL pre-$label missing '$marker'"
    echo "$out" | tail -6 | sed 's/^/      /'
  fi
done

get_sid() {
  dc --json pane list 2>/dev/null | python3 -c "
import sys,json
panes=json.load(sys.stdin)
for p in panes:
    if p.get('id','').startswith('$1'):
        print(p.get('agent_session_id','-'))
        break
"
}

echo "== capture pre-restart session-ids"
declare -a PRE_SIDS
for pid in "${PANE_IDS[@]}"; do
  PRE_SIDS+=("$(get_sid $pid)")
done

echo "== sleep 10 for autosave"
sleep 10

echo "== HARD KILL"
dev_kill_hard

echo "== restart dev"
dev_start_bg
dev_wait_ready 180 || exit 1
echo "== wait 12s for resume scripts to run"
sleep 12

echo "== POST-restart verification"
PASS=0
FAIL=0
for i in "${!PANE_IDS[@]}"; do
  pid="${PANE_IDS[$i]}"
  label="${PANE_LABELS[$i]}"
  marker="${PANE_MARKERS[$i]}"
  pre_sid="${PRE_SIDS[$i]}"
  out=$(dc tab get-text --tab "$pid" 2>&1 || echo "")
  post_sid=$(get_sid $pid)
  errors=""
  if echo "$out" | grep -q '\[Process exited\]'; then
    errors+=" PROCESS_EXITED"
  fi
  if [ "$pre_sid" != "-" ] && [ -n "$pre_sid" ] && [ "$pre_sid" != "$post_sid" ]; then
    errors+=" SID_LOST(pre=$pre_sid post=$post_sid)"
  fi
  if [ -n "$marker" ] && ! echo "$out" | grep -q "$marker"; then
    errors+=" MARKER_MISSING($marker)"
  fi
  if [ -z "$errors" ]; then
    echo "  PASS $label"
    PASS=$((PASS+1))
  else
    echo "  FAIL $label:$errors"
    echo "$out" | tail -6 | sed 's/^/      /'
    FAIL=$((FAIL+1))
  fi
done

echo ""
echo "== INPUT-PARSER scenario: typed launch command in a plain shell"
WS_TYPED=$(new_workspace WS-typed-codex "$CWD_BASE/typed-codex")
P_TYPED=$(new_shell_pane $WS_TYPED "$CWD_BASE/typed-codex" typed-codex)
echo "  pane: $P_TYPED in ws $WS_TYPED"
sleep 4
dc send "codex --help" --tab "$P_TYPED" >/dev/null 2>&1 || true
sleep 2
TYPED_AT=$(dc --json pane list 2>/dev/null | python3 -c "
import sys,json
panes=json.load(sys.stdin)
for p in panes:
    if p.get('id','').startswith('$P_TYPED'):
        print(p.get('agent_type','-'))
        break
")
if [ "$TYPED_AT" = "codex" ]; then
  echo "  PASS typed-codex: agent_type=codex"
  PASS=$((PASS+1))
else
  echo "  FAIL typed-codex: agent_type=$TYPED_AT (expected 'codex')"
  FAIL=$((FAIL+1))
fi

echo ""
echo "== cleanup: closing WS-* test workspaces"
cleanup_test_workspaces WS- 2>/dev/null

echo "== SUMMARY: scale=$SCALE  $PASS pass, $FAIL fail"
exit $FAIL
