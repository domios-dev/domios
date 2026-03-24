#!/bin/bash
# llm-eval.sh — LLM agent eval: verify Claude can use CodeTerm skill correctly
# Spawns real Claude agents with natural language tasks, checks results.
# Usage: CODETERM_SOCKET_PATH=/tmp/codeterm-PID.sock ./tests/llm-eval.sh

set -o pipefail

CT="cli/target/release/codeterm"
PASS=0
FAIL=0

pass() { echo "  PASS: $1"; ((PASS++)); }
fail() { echo "  FAIL: $1"; ((FAIL++)); }

echo "╔══════════════════════════════════════╗"
echo "║   CodeTerm LLM Eval Suite            ║"
echo "╚══════════════════════════════════════╝"

$CT identify >/dev/null 2>&1 || { echo "ERROR: Cannot connect. Set CODETERM_SOCKET_PATH."; exit 1; }
echo "Connected."

# Cleanup
echo ""
echo "=== Cleanup ==="
for ws in $($CT workspace list --json 2>/dev/null | python3 -c "import json,sys; [print(w['id']) for w in json.load(sys.stdin)]" 2>/dev/null); do
    $CT workspace close "$ws" 2>/dev/null || true
done
sleep 2
$CT workspace create "LLM Eval" 2>/dev/null
sleep 3

# Helper: spawn Claude, wait for completion, check result
# Usage: run_llm_test "test name" "task for claude" "check_function"
WAIT_SECS=90

# ═══════════════════════════════════════════════════════════════
echo ""
echo "══ Test 1: Rename tab ══"
echo "Claude should rename its tab using codeterm CLI"

T1=$($CT agent spawn claude --title "Renamer" --task 'Rename your tab to "My Cool Tab" using the codeterm CLI. Just run the rename command and nothing else.' --json 2>&1 | python3 -c "import json,sys; print(json.load(sys.stdin).get('pane_id',''))" 2>/dev/null)
echo "  Spawned: $T1"
sleep $WAIT_SECS

TITLE=$($CT pane list --json 2>&1 | python3 -c "
import json,sys
for p in json.load(sys.stdin):
    if p['id'] == '$T1': print(p.get('title',''))
" 2>/dev/null)
if echo "$TITLE" | grep -qi "cool"; then
    pass "Tab renamed to: $TITLE"
else
    fail "Tab not renamed. Title: $TITLE"
fi

# ═══════════════════════════════════════════════════════════════
echo ""
echo "══ Test 2: Conductor + 2 Workers ══"
echo "Claude should set up orchestrator hierarchy"

T2=$($CT agent spawn claude --title "Boss" --can-spawn \
  --task 'You are running inside CodeTerm. Set yourself as conductor using $CODETERM_PANE_ID, then spawn 2 workers:
- Worker "Alpha" with cwd /tmp and task "echo alpha ready"
- Worker "Beta" with cwd /tmp and task "echo beta ready"
Use codeterm agent spawn for each. Then run codeterm agent hierarchy to verify.' \
  --json 2>&1 | python3 -c "import json,sys; print(json.load(sys.stdin).get('pane_id',''))" 2>/dev/null)
echo "  Spawned conductor: $T2"
sleep $WAIT_SECS

# Check hierarchy
HIER=$($CT agent hierarchy 2>&1)
echo "$HIER"
HAS_CONDUCTOR=$(echo "$HIER" | grep -c "Conductor")
HAS_ALPHA=$(echo "$HIER" | grep -ci "alpha")
HAS_BETA=$(echo "$HIER" | grep -ci "beta")

if [ "$HAS_CONDUCTOR" -ge 1 ] && [ "$HAS_ALPHA" -ge 1 ] && [ "$HAS_BETA" -ge 1 ]; then
    pass "Conductor + Alpha + Beta hierarchy created"
else
    fail "Hierarchy incomplete: conductors=$HAS_CONDUCTOR alpha=$HAS_ALPHA beta=$HAS_BETA"
fi

# ═══════════════════════════════════════════════════════════════
echo ""
echo "══ Test 3: Cross-workspace spawn ══"
echo "Claude should create a workspace and spawn agent there"

T3=$($CT agent spawn claude --title "WS-Creator" \
  --task 'Create a new workspace called "Remote Project" using codeterm workspace create. Then spawn a Claude agent in that workspace using codeterm agent spawn with --workspace flag and title "Remote Agent" and task "echo hello". Report the workspace ID and agent pane ID.' \
  --json 2>&1 | python3 -c "import json,sys; print(json.load(sys.stdin).get('pane_id',''))" 2>/dev/null)
echo "  Spawned: $T3"
sleep $WAIT_SECS

# Check if "Remote Project" workspace exists with content
WS_EXISTS=$($CT workspace list --json 2>&1 | python3 -c "
import json,sys
for w in json.load(sys.stdin):
    if 'remote' in w['name'].lower() or 'Remote' in w['name']:
        print(f\"{w['name']} tabs={w['tab_count']}\")
" 2>/dev/null)
if [ -n "$WS_EXISTS" ]; then
    pass "Cross-workspace: $WS_EXISTS"
else
    fail "Remote workspace not created"
fi

# ═══════════════════════════════════════════════════════════════
echo ""
echo "══ Test 4: Split pane ══"
echo "Claude should split a pane and send a command to the new pane"

T4=$($CT agent spawn claude --title "Splitter" \
  --task 'Split your current pane to the right using codeterm pane split. Get the new pane ID with codeterm pane current --last. Then send the text "echo SPLIT_OK > /tmp/ct-llm-split.txt" to that new pane using codeterm send-text. Report what you did.' \
  --json 2>&1 | python3 -c "import json,sys; print(json.load(sys.stdin).get('pane_id',''))" 2>/dev/null)
echo "  Spawned: $T4"
sleep $WAIT_SECS

if [ -f /tmp/ct-llm-split.txt ] && grep -q "SPLIT_OK" /tmp/ct-llm-split.txt; then
    pass "Split + send-text to new pane worked"
else
    fail "Split test file not created"
fi
rm -f /tmp/ct-llm-split.txt

# ═══════════════════════════════════════════════════════════════
echo ""
echo "══ Test 5: Notification ══"
echo "Claude should send a notification"

T5=$($CT agent spawn claude --title "Notifier" \
  --task 'Send a notification using codeterm notify with title "Test Alert" level "info" and body "LLM eval passed". Just the notify command, nothing else.' \
  --json 2>&1 | python3 -c "import json,sys; print(json.load(sys.stdin).get('pane_id',''))" 2>/dev/null)
echo "  Spawned: $T5"
sleep $WAIT_SECS

NOTIFS=$($CT notifications --unread 2>&1)
if echo "$NOTIFS" | grep -qi "test alert\|eval passed"; then
    pass "Notification sent"
else
    fail "Notification not found: $NOTIFS"
fi

# ═══════════════════════════════════════════════════════════════
echo ""
echo "╔══════════════════════════════════════╗"
echo "║  LLM Results: $PASS passed, $FAIL failed      ║"
echo "╚══════════════════════════════════════╝"

rm -f /tmp/ct-llm-*.txt
exit $FAIL
