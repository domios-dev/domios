#!/bin/bash
# llm-natural-eval.sh — LLM eval with natural language prompts (no command hints)
# Tests that Claude can use the CodeTerm skill from natural human requests.
# Usage: CODETERM_SOCKET_PATH=/tmp/codeterm-PID.sock ./tests/llm-natural-eval.sh

set -o pipefail

CT="cli/target/release/codeterm"
PASS=0
FAIL=0

pass() { echo "  PASS: $1"; ((PASS++)); }
fail() { echo "  FAIL: $1"; ((FAIL++)); }

WAIT=90

echo "╔══════════════════════════════════════╗"
echo "║  LLM Natural Language Eval           ║"
echo "╚══════════════════════════════════════╝"

$CT identify >/dev/null 2>&1 || { echo "ERROR: Cannot connect."; exit 1; }

# Cleanup
for ws in $($CT workspace list --json 2>/dev/null | python3 -c "import json,sys; [print(w['id']) for w in json.load(sys.stdin)]" 2>/dev/null); do
    $CT workspace close "$ws" 2>/dev/null || true
done
sleep 2
$CT workspace create "Eval" 2>/dev/null
sleep 3

# ═══════════════════════════════════════════════════════════════
echo ""
echo "══ Test 1: Natural split request ══"
echo "   Prompt: 'split the terminal please'"

T1=$($CT agent spawn claude --title "T1" \
  --task 'split the terminal please' \
  --json 2>&1 | python3 -c "import json,sys; print(json.load(sys.stdin).get('pane_id',''))" 2>/dev/null)
echo "  Spawned: $T1"
sleep $WAIT

PANE_COUNT=$($CT pane list --json 2>&1 | python3 -c "import json,sys; print(len(json.load(sys.stdin)))")
# Should have at least 3 panes (original eval + T1 + split from T1)
if [ "$PANE_COUNT" -ge 3 ]; then
    pass "Split created ($PANE_COUNT panes total)"
else
    fail "Expected >=3 panes, got $PANE_COUNT"
fi

# ═══════════════════════════════════════════════════════════════
echo ""
echo "══ Test 2: Create workspace for project ══"
echo "   Prompt: 'create a workspace called MyProject'"

T2=$($CT agent spawn claude --title "T2" \
  --task 'create a workspace called MyProject' \
  --json 2>&1 | python3 -c "import json,sys; print(json.load(sys.stdin).get('pane_id',''))" 2>/dev/null)
echo "  Spawned: $T2"
sleep $WAIT

HAS_WS=$($CT workspace list --json 2>&1 | python3 -c "
import json,sys
for w in json.load(sys.stdin):
    if 'myproject' in w['name'].lower() or 'MyProject' in w['name']:
        print('yes')
        break
" 2>/dev/null)
if [ "$HAS_WS" = "yes" ]; then
    pass "Workspace 'MyProject' created"
else
    WS_NAMES=$($CT workspace list --json 2>&1 | python3 -c "import json,sys; print([w['name'] for w in json.load(sys.stdin)])")
    fail "No MyProject workspace. Found: $WS_NAMES"
fi

# ═══════════════════════════════════════════════════════════════
echo ""
echo "══ Test 3: Orchestrator natural language ══"
echo "   Prompt: 'set yourself up as orchestrator and spawn two workers'"

T3=$($CT agent spawn claude --title "T3" --can-spawn \
  --task 'set yourself up as orchestrator and spawn two workers — one called "frontend" and another called "backend". give each of them a simple task like listing files' \
  --json 2>&1 | python3 -c "import json,sys; print(json.load(sys.stdin).get('pane_id',''))" 2>/dev/null)
echo "  Spawned: $T3"
sleep $WAIT

HIER=$($CT agent hierarchy 2>&1)
echo "$HIER"
HAS_FRONT=$(echo "$HIER" | grep -ci "frontend\|front")
HAS_BACK=$(echo "$HIER" | grep -ci "backend\|back")
HAS_COND=$(echo "$HIER" | grep -c "Conductor")
if [ "$HAS_COND" -ge 1 ] && [ "$HAS_FRONT" -ge 1 ] && [ "$HAS_BACK" -ge 1 ]; then
    pass "Orchestrator with frontend + backend workers"
else
    fail "Hierarchy incomplete: conductors=$HAS_COND front=$HAS_FRONT back=$HAS_BACK"
fi

# ═══════════════════════════════════════════════════════════════
echo ""
echo "══ Test 4: Send command to another pane ══"
echo "   Prompt: 'run echo hello in a new pane'"

T4=$($CT agent spawn claude --title "T4" \
  --task 'open a new pane by splitting and run "echo HELLO_FROM_SPLIT > /tmp/ct-natural-hello.txt" in that new pane' \
  --json 2>&1 | python3 -c "import json,sys; print(json.load(sys.stdin).get('pane_id',''))" 2>/dev/null)
echo "  Spawned: $T4"
sleep $WAIT

if [ -f /tmp/ct-natural-hello.txt ] && grep -q "HELLO_FROM_SPLIT" /tmp/ct-natural-hello.txt; then
    pass "Command executed in split pane"
else
    fail "File not created from split pane"
fi
rm -f /tmp/ct-natural-hello.txt

# ═══════════════════════════════════════════════════════════════
echo ""
echo "══ Test 5: Rename tab naturally ══"
echo "   Prompt: 'rename this tab to debug session'"

T5=$($CT agent spawn claude --title "T5" \
  --task 'rename this tab to "debug session"' \
  --json 2>&1 | python3 -c "import json,sys; print(json.load(sys.stdin).get('pane_id',''))" 2>/dev/null)
echo "  Spawned: $T5"
sleep $WAIT

TITLE=$($CT pane list --json 2>&1 | python3 -c "
import json,sys
for p in json.load(sys.stdin):
    if p['id'] == '$T5': print(p.get('title',''))
" 2>/dev/null)
if echo "$TITLE" | grep -qi "debug"; then
    pass "Tab renamed to: $TITLE"
else
    fail "Tab not renamed. Title: $TITLE"
fi

# ═══════════════════════════════════════════════════════════════
echo ""
echo "══ Test 6: Notify me when done ══"
echo "   Prompt: 'send me a notification saying all tests passed'"

T6=$($CT agent spawn claude --title "T6" \
  --task 'send me a notification saying "all tests passed"' \
  --json 2>&1 | python3 -c "import json,sys; print(json.load(sys.stdin).get('pane_id',''))" 2>/dev/null)
echo "  Spawned: $T6"
sleep $WAIT

NOTIFS=$($CT notifications --unread 2>&1)
if echo "$NOTIFS" | grep -qi "tests passed\|all tests"; then
    pass "Notification received"
else
    fail "Notification not found"
fi

# ═══════════════════════════════════════════════════════════════
echo ""
echo "══ Test 7: Create workspace and spawn agent there ══"
echo "   Prompt: 'open a separate workspace for testing and put an agent in it'"

T7=$($CT agent spawn claude --title "T7" \
  --task 'create a new workspace called "Testing" and spawn a claude agent inside it with the task "echo ready"' \
  --json 2>&1 | python3 -c "import json,sys; print(json.load(sys.stdin).get('pane_id',''))" 2>/dev/null)
echo "  Spawned: $T7"
sleep $WAIT

HAS_TESTING=$($CT workspace list --json 2>&1 | python3 -c "
import json,sys
for w in json.load(sys.stdin):
    if 'testing' in w['name'].lower():
        print(f\"{w['name']} tabs={w['tab_count']}\")
" 2>/dev/null)
if [ -n "$HAS_TESTING" ]; then
    pass "Testing workspace with agent: $HAS_TESTING"
else
    fail "Testing workspace not created"
fi

# ═══════════════════════════════════════════════════════════════
echo ""
echo "╔══════════════════════════════════════╗"
echo "║  Natural LLM: $PASS passed, $FAIL failed     ║"
echo "╚══════════════════════════════════════╝"

rm -f /tmp/ct-natural-*.txt
exit $FAIL
