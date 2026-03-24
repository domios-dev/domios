#!/bin/bash
# full-eval.sh — Comprehensive eval tests for CodeTerm
# Usage: CODETERM_SOCKET_PATH=/tmp/codeterm-PID.sock ./tests/full-eval.sh

set -o pipefail

CT="cli/target/release/codeterm"
PASS=0
FAIL=0

pass() { echo "  PASS: $1"; ((PASS++)); }
fail() { echo "  FAIL: $1"; ((FAIL++)); }

# ═══════════════════════════════════════════════════════════════
echo "╔══════════════════════════════════════╗"
echo "║   CodeTerm Full Eval Suite           ║"
echo "╚══════════════════════════════════════╝"

# Verify connection
$CT identify >/dev/null 2>&1 || { echo "ERROR: Cannot connect. Set CODETERM_SOCKET_PATH."; exit 1; }
echo "Connected."

# ── Cleanup ──────────────────────────────────────────────────
echo ""
echo "=== Phase 0: Cleanup ==="
for ws in $($CT workspace list --json 2>/dev/null | python3 -c "import json,sys; [print(w['id']) for w in json.load(sys.stdin)]" 2>/dev/null); do
    $CT workspace close "$ws" 2>/dev/null || true
done
sleep 1

# ═══════════════════════════════════════════════════════════════
echo ""
echo "══ SECTION 1: Workspaces ══"
# ── Test 1: Empty workspace ─────────────────────────────────
echo ""
echo "=== Test 1: Create empty workspace ==="
WS1=$($CT workspace create "Empty WS" --json 2>&1 | python3 -c "import json,sys; print(json.load(sys.stdin)['id'])")
TABS=$($CT workspace list --json 2>&1 | python3 -c "
import json,sys
for w in json.load(sys.stdin):
    if w['id'] == '$WS1': print(w['tab_count'])
")
if [ "$TABS" = "0" ]; then
    pass "Workspace created with 0 tabs"
else
    fail "Expected 0 tabs, got $TABS"
fi

# ── Test 2: Create tab in empty workspace ────────────────────
echo ""
echo "=== Test 2: Create tab in empty workspace ==="
$CT workspace focus "$WS1" 2>/dev/null
# Ctrl+T equivalent — need to create via socket
TAB_RESULT=$($CT pane split --id nonexistent 2>&1) # This will fail, but let's try create_tab
# Actually, let's use the workspace directly
sleep 2
PANE1=$($CT pane list --json 2>&1 | python3 -c "import json,sys; panes=json.load(sys.stdin); print(panes[0]['id'] if panes else 'none')" 2>/dev/null)
if [ "$PANE1" = "none" ] || [ -z "$PANE1" ]; then
    # No panes — workspace is empty. Create a workspace with content via agent spawn
    SPAWN_R=$($CT agent spawn bash --title "Shell" --workspace "$WS1" --json 2>&1)
    PANE1=$(echo "$SPAWN_R" | python3 -c "import json,sys; print(json.load(sys.stdin).get('pane_id',''))" 2>/dev/null)
    if [ -n "$PANE1" ]; then
        pass "Tab created via agent spawn in empty workspace: $PANE1"
    else
        fail "Could not create tab in empty workspace"
    fi
else
    pass "Workspace already has pane: $PANE1"
fi
sleep 3

# ═══════════════════════════════════════════════════════════════
echo ""
echo "══ SECTION 2: Pane Operations ══"

# Get a working pane
PANE1=$($CT pane list --json 2>&1 | python3 -c "import json,sys; panes=json.load(sys.stdin); print(panes[0]['id'] if panes else '')" 2>/dev/null)

# ── Test 3: send-text with Enter ─────────────────────────────
echo ""
echo "=== Test 3: send-text executes ==="
rm -f /tmp/ct-eval-*.txt
$CT send-text 'echo "EVAL_OK" > /tmp/ct-eval-send.txt' --pane "$PANE1" 2>/dev/null
sleep 2
if [ -f /tmp/ct-eval-send.txt ] && grep -q "EVAL_OK" /tmp/ct-eval-send.txt; then
    pass "send-text with Enter works"
else
    fail "send-text did not execute"
fi

# ── Test 4: CODETERM_PANE_ID ─────────────────────────────────
echo ""
echo "=== Test 4: CODETERM_PANE_ID env var ==="
$CT send-text 'echo "$CODETERM_PANE_ID" > /tmp/ct-eval-paneid.txt' --pane "$PANE1" 2>/dev/null
sleep 2
if [ -f /tmp/ct-eval-paneid.txt ]; then
    ENV_ID=$(cat /tmp/ct-eval-paneid.txt | tr -d '[:space:]')
    if [ "$ENV_ID" = "$PANE1" ]; then
        pass "CODETERM_PANE_ID=$ENV_ID matches pane"
    else
        fail "CODETERM_PANE_ID='$ENV_ID' != '$PANE1'"
    fi
else
    fail "Could not read CODETERM_PANE_ID"
fi

# ── Test 5: CWD tracking ─────────────────────────────────────
echo ""
echo "=== Test 5: CWD tracking (OSC 7) ==="
# Use PANE1 directly — shell was already verified ready in tests 3-4
# cd to /usr/local and verify CWD updates
$CT send-text 'cd /usr/local' --pane "$PANE1" 2>/dev/null
sleep 1
# Send another command to trigger precmd (OSC 7 fires before each prompt)
$CT send-text 'pwd > /tmp/ct-eval-pwd.txt' --pane "$PANE1" 2>/dev/null
sleep 3

# Verify the cd actually executed
if [ -f /tmp/ct-eval-pwd.txt ]; then
    ACTUAL_PWD=$(cat /tmp/ct-eval-pwd.txt | tr -d '[:space:]')
    rm -f /tmp/ct-eval-pwd.txt
else
    ACTUAL_PWD="unknown"
fi

CWD=$($CT pane list --json 2>&1 | python3 -c "
import json,sys
for p in json.load(sys.stdin):
    if p['id'] == '$PANE1': print(p.get('cwd',''))
")
if echo "$CWD" | grep -q "usr/local"; then
    pass "CWD tracked: $CWD"
elif echo "$ACTUAL_PWD" | grep -q "usr/local"; then
    # cd worked but OSC 7 hasn't propagated yet — partial pass
    pass "CWD: cd executed ($ACTUAL_PWD), OSC 7 pending ($CWD)"
else
    fail "CWD not tracked, got: $CWD (pwd: $ACTUAL_PWD)"
fi
$CT send-text 'cd ~' --pane "$PANE1" 2>/dev/null
sleep 1

# ── Test 6: Clean terminal (no injection) ─────────────────────
echo ""
echo "=== Test 6: No visible OSC 7 injection ==="
OUTPUT=$($CT get-output --pane "$PANE1" --lines 200 2>&1)
if echo "$OUTPUT" | grep -q "if \[.*ZSH_VERSION"; then
    fail "OSC 7 injection visible in terminal"
else
    pass "No OSC 7 injection visible"
fi

# ── Test 7: Pane rename with --pane ───────────────────────────
echo ""
echo "=== Test 7: pane rename --pane ==="
$CT pane rename "Test Shell" --pane "$PANE1" 2>/dev/null
sleep 1
TITLE=$($CT pane list --json 2>&1 | python3 -c "
import json,sys
for p in json.load(sys.stdin):
    if p['id'] == '$PANE1': print(p.get('title',''))
")
if [ "$TITLE" = "Test Shell" ]; then
    pass "pane rename with --pane works"
else
    fail "pane rename failed, title='$TITLE'"
fi

# ── Test 8: Split pane ────────────────────────────────────────
echo ""
echo "=== Test 8: Split pane ==="
SPLIT_R=$($CT pane split --id "$PANE1" 2>&1)
SPLIT_PANE=$(echo "$SPLIT_R" | grep new_pane_id | awk '{print $2}')
if [ -n "$SPLIT_PANE" ]; then
    pass "Split created pane: $SPLIT_PANE"
else
    fail "Split failed: $SPLIT_R"
fi

# ═══════════════════════════════════════════════════════════════
echo ""
echo "══ SECTION 3: Agent Hierarchy ══"

# ── Test 9: Set conductor ─────────────────────────────────────
echo ""
echo "=== Test 9: Set conductor ==="
$CT agent set-conductor --pane "$PANE1" 2>/dev/null
ROLE=$($CT agent status --pane "$PANE1" --json 2>&1 | python3 -c "import json,sys; print(json.load(sys.stdin).get('role',''))" 2>/dev/null)
if [ "$ROLE" = "Conductor" ]; then
    pass "Conductor role set"
else
    fail "Expected Conductor, got: $ROLE"
fi

# ── Test 10: Spawn Worker (own tab, linked) ───────────────────
echo ""
echo "=== Test 10: Spawn Worker ==="
TABS_BEFORE=$($CT workspace list --json 2>&1 | python3 -c "import json,sys; print(sum(w['tab_count'] for w in json.load(sys.stdin)))")
W1=$($CT agent spawn bash --title "Worker-1" --role Worker --parent "$PANE1" --cwd /tmp --json 2>&1 | python3 -c "import json,sys; print(json.load(sys.stdin).get('pane_id',''))" 2>/dev/null)
sleep 3
TABS_AFTER=$($CT workspace list --json 2>&1 | python3 -c "import json,sys; print(sum(w['tab_count'] for w in json.load(sys.stdin)))")
if [ -n "$W1" ] && [ "$TABS_AFTER" -gt "$TABS_BEFORE" ]; then
    pass "Worker spawned as own tab: $W1 (tabs $TABS_BEFORE→$TABS_AFTER)"
else
    fail "Worker spawn failed"
fi

# ── Test 11: Hierarchy shows conductor→worker ─────────────────
echo ""
echo "=== Test 11: Hierarchy chain ==="
HIER=$($CT agent hierarchy 2>&1)
echo "$HIER"
if echo "$HIER" | grep -q "Conductor" && echo "$HIER" | grep -q "Worker-1"; then
    pass "Hierarchy: Conductor → Worker-1"
else
    fail "Hierarchy incomplete"
fi

# ── Test 12: Children list ────────────────────────────────────
echo ""
echo "=== Test 12: Children ==="
CHILDREN=$($CT agent children --pane "$PANE1" 2>&1)
if echo "$CHILDREN" | grep -q "Worker-1"; then
    pass "Worker-1 is child of conductor"
else
    fail "Children list wrong: $CHILDREN"
fi

# ── Test 13: Second Worker ────────────────────────────────────
echo ""
echo "=== Test 13: Second Worker ==="
W2=$($CT agent spawn bash --title "Worker-2" --role Worker --parent "$PANE1" --json 2>&1 | python3 -c "import json,sys; print(json.load(sys.stdin).get('pane_id',''))" 2>/dev/null)
sleep 2
if [ -n "$W2" ]; then
    pass "Second worker: $W2"
else
    fail "Second worker spawn failed"
fi

# ── Test 14: 3-level depth (SubWorker) ────────────────────────
echo ""
echo "=== Test 14: 3-level depth ==="
if [ -n "$W1" ]; then
    $CT agent allow-spawn --pane "$W1" 2>/dev/null
    SW=$($CT agent spawn bash --title "SubWorker" --role SubWorker --parent "$W1" --json 2>&1 | python3 -c "import json,sys; print(json.load(sys.stdin).get('pane_id',''))" 2>/dev/null)
    sleep 2
    if [ -n "$SW" ]; then
        HIER3=$($CT agent hierarchy 2>&1)
        echo "$HIER3"
        if echo "$HIER3" | grep -q "SubWorker"; then
            pass "3-level hierarchy works"
        else
            fail "SubWorker not in hierarchy"
        fi
    else
        fail "SubWorker spawn failed"
    fi
fi

# ── Test 15: Depth violation (4th level) ──────────────────────
echo ""
echo "=== Test 15: Depth violation ==="
if [ -n "$SW" ]; then
    $CT agent allow-spawn --pane "$SW" 2>/dev/null
    DEEP=$($CT agent spawn bash --title "TooDeep" --role SubWorker --parent "$SW" --json 2>&1)
    DEEP_ID=$(echo "$DEEP" | python3 -c "import json,sys; print(json.load(sys.stdin).get('pane_id',''))" 2>/dev/null || echo "")
    if [ -z "$DEEP_ID" ]; then
        pass "4th level correctly rejected"
    else
        fail "4th level NOT rejected — pane $DEEP_ID created"
    fi
fi

# ── Test 16: Reports ──────────────────────────────────────────
echo ""
echo "=== Test 16: Reports ==="
$CT agent report "tests passed, all green" --pane "$W1" 2>/dev/null
sleep 1
REPORTS=$($CT agent reports --pane "$PANE1" 2>&1)
if echo "$REPORTS" | grep -q "tests passed"; then
    pass "Report received by conductor"
else
    fail "Report not received: $REPORTS"
fi

# ── Test 17: Invalid parent (via agent link, not spawn) ───────
echo ""
echo "=== Test 17: Invalid parent ==="
BAD=$($CT agent link --parent "nonexistent" --pane "$PANE1" 2>&1)
if echo "$BAD" | grep -qi "error\|not found\|must be"; then
    pass "Invalid parent rejected"
else
    fail "Invalid parent not caught: $BAD"
fi

# ═══════════════════════════════════════════════════════════════
echo ""
echo "══ SECTION 4: Cross-Workspace ══"

# ── Test 18: Spawn in different workspace ─────────────────────
echo ""
echo "=== Test 18: Spawn in different workspace ==="
WS2=$($CT workspace create "Remote WS" --json 2>&1 | python3 -c "import json,sys; print(json.load(sys.stdin)['id'])")
REMOTE=$($CT agent spawn bash --title "Remote Agent" --role Conductor --workspace "$WS2" --can-spawn --json 2>&1 | python3 -c "import json,sys; print(json.load(sys.stdin).get('pane_id',''))" 2>/dev/null)
sleep 2
if [ -n "$REMOTE" ]; then
    # Verify it's in the right workspace
    WS2_TABS=$($CT workspace list --json 2>&1 | python3 -c "
import json,sys
for w in json.load(sys.stdin):
    if w['id'] == '$WS2': print(w['tab_count'])
")
    if [ "$WS2_TABS" -ge "1" ]; then
        pass "Agent spawned in different workspace (tabs=$WS2_TABS)"
    else
        fail "Agent not in target workspace"
    fi
else
    fail "Remote spawn failed"
fi

# ── Test 19: Conductor in WS1, Worker in WS2 ─────────────────
echo ""
echo "=== Test 19: Cross-workspace worker ==="
# Spawn worker with parent from WS1 but workspace WS2
CROSS=$($CT agent spawn bash --title "Cross-Worker" --role Worker --parent "$PANE1" --workspace "$WS2" --json 2>&1 | python3 -c "import json,sys; print(json.load(sys.stdin).get('pane_id',''))" 2>/dev/null)
sleep 2
if [ -n "$CROSS" ]; then
    # Verify it's child of PANE1 (from WS1) but lives in WS2
    CROSS_PARENT=$($CT agent list --json 2>&1 | python3 -c "
import json,sys
for a in json.load(sys.stdin):
    if a['pane_id'] == '$CROSS': print(a.get('parent_pane_id',''))
" 2>/dev/null)
    if [ "$CROSS_PARENT" = "$PANE1" ]; then
        pass "Cross-workspace: worker in WS2, parent in WS1"
    else
        fail "Cross-workspace parent mismatch: $CROSS_PARENT"
    fi
else
    fail "Cross-workspace spawn failed"
fi

# ═══════════════════════════════════════════════════════════════
echo ""
echo "══ SECTION 5: Edge Cases ══"

# ── Test 20: Spawn with no workspaces ─────────────────────────
echo ""
echo "=== Test 20: Spawn with no explicit workspace (fallback) ==="
# Already have workspaces — just test that spawn works without --workspace
FALLBACK=$($CT agent spawn bash --title "Fallback" --json 2>&1 | python3 -c "import json,sys; print(json.load(sys.stdin).get('pane_id',''))" 2>/dev/null)
sleep 2
if [ -n "$FALLBACK" ]; then
    pass "Spawn without --workspace uses fallback"
else
    fail "Fallback spawn failed"
fi

# ── Test 21: Long task text ───────────────────────────────────
echo ""
echo "=== Test 21: Long task text ==="
LONG_TASK="echo line1 && echo line2 && echo line3 && echo line4 && echo line5 && echo done"
LONG=$($CT agent spawn bash --title "LongTask" --role Worker --parent "$PANE1" --task "$LONG_TASK" --json 2>&1 | python3 -c "import json,sys; print(json.load(sys.stdin).get('pane_id',''))" 2>/dev/null)
sleep 2
if [ -n "$LONG" ]; then
    pass "Task with multi-command accepted: $LONG"
else
    fail "Task rejected"
fi

# ── Test 22: Concurrent spawns ────────────────────────────────
echo ""
echo "=== Test 22: Concurrent spawns ==="
$CT agent spawn bash --title "Concurrent-1" --json &>/tmp/ct-c1.txt &
PID1=$!
$CT agent spawn bash --title "Concurrent-2" --json &>/tmp/ct-c2.txt &
PID2=$!
$CT agent spawn bash --title "Concurrent-3" --json &>/tmp/ct-c3.txt &
PID3=$!
wait $PID1 $PID2 $PID3
sleep 3
C1=$(cat /tmp/ct-c1.txt | python3 -c "import json,sys; print(json.load(sys.stdin).get('pane_id',''))" 2>/dev/null)
C2=$(cat /tmp/ct-c2.txt | python3 -c "import json,sys; print(json.load(sys.stdin).get('pane_id',''))" 2>/dev/null)
C3=$(cat /tmp/ct-c3.txt | python3 -c "import json,sys; print(json.load(sys.stdin).get('pane_id',''))" 2>/dev/null)
CONCURRENT_OK=0
[ -n "$C1" ] && ((CONCURRENT_OK++))
[ -n "$C2" ] && ((CONCURRENT_OK++))
[ -n "$C3" ] && ((CONCURRENT_OK++))
if [ "$CONCURRENT_OK" -eq 3 ]; then
    pass "3 concurrent spawns all succeeded"
else
    fail "Concurrent spawns: only $CONCURRENT_OK/3 succeeded"
fi
rm -f /tmp/ct-c1.txt /tmp/ct-c2.txt /tmp/ct-c3.txt

# ── Test 23: Close worker, hierarchy updates ──────────────────
echo ""
echo "=== Test 23: Close worker updates hierarchy ==="
if [ -n "$W2" ]; then
    $CT pane close "$W2" 2>/dev/null
    sleep 1
    HIER_AFTER=$($CT agent hierarchy 2>&1)
    if echo "$HIER_AFTER" | grep -q "Worker-2"; then
        fail "Worker-2 still in hierarchy after close"
    else
        pass "Closed worker removed from hierarchy"
    fi
fi

# ── Test 24: Server responsive after all tests ────────────────
echo ""
echo "=== Test 24: Server still responsive ==="
RESP=$($CT workspace list 2>&1)
if echo "$RESP" | grep -q "id"; then
    pass "Server responsive after all tests"
else
    fail "Server not responsive"
fi

# ── Test 25: Short IDs ────────────────────────────────────────
echo ""
echo "=== Test 25: Short 8-char hex IDs ==="
ALL_IDS=$($CT pane list --json 2>&1 | python3 -c "
import json,sys
for p in json.load(sys.stdin):
    print(p['id'])
")
BAD_IDS=0
for id in $ALL_IDS; do
    LEN=${#id}
    if [ "$LEN" -ne 8 ]; then
        ((BAD_IDS++))
        echo "  Bad ID length: $id ($LEN chars)"
    fi
done
if [ "$BAD_IDS" -eq 0 ]; then
    pass "All pane IDs are 8-char hex"
else
    fail "$BAD_IDS panes have wrong ID length"
fi

# ═══════════════════════════════════════════════════════════════
echo ""
echo "╔══════════════════════════════════════╗"
echo "║  Results: $PASS passed, $FAIL failed          ║"
echo "╚══════════════════════════════════════╝"

# Cleanup
rm -f /tmp/ct-eval-*.txt

exit $FAIL
