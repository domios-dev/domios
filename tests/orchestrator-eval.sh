#!/bin/bash
# orchestrator-eval.sh — Systematic eval tests for CodeTerm agent orchestrator
# Run against a live CodeTerm instance.
# Usage: CODETERM_SOCKET_PATH=/tmp/codeterm-PID.sock ./tests/orchestrator-eval.sh

set -uo pipefail
# Don't exit on error — we want to run all tests

CT="cli/target/release/codeterm"
PASS=0
FAIL=0
SKIP=0

pass() { echo "  PASS: $1"; ((PASS++)); }
fail() { echo "  FAIL: $1"; ((FAIL++)); }
skip() { echo "  SKIP: $1"; ((SKIP++)); }

# Verify CLI and connection
echo "=== Phase 0: Setup ==="
$CT identify >/dev/null 2>&1 || { echo "ERROR: Cannot connect to CodeTerm. Set CODETERM_SOCKET_PATH."; exit 1; }
echo "Connected to CodeTerm."

# Close ALL workspaces for clean state
echo "Cleaning workspaces..."
WS_IDS=$($CT workspace list --json 2>/dev/null | python3 -c "
import json,sys
ws = json.load(sys.stdin)
for w in ws:
    print(w['id'])
" 2>/dev/null || true)

for ws in $WS_IDS; do
    $CT workspace close "$ws" 2>/dev/null || true
done

# After closing all, CodeTerm should auto-create a default workspace
sleep 2
WS_COUNT=$($CT workspace list --json 2>/dev/null | python3 -c "import json,sys; print(len(json.load(sys.stdin)))")
echo "Workspaces after cleanup: $WS_COUNT"

# ─── Test 1: Basic workspace + pane ─────────────────────────────
echo ""
echo "=== Test 1: Create workspace ==="
WS_JSON=$($CT workspace create "Test Workspace" --json 2>&1)
WS_ID=$(echo "$WS_JSON" | python3 -c "import json,sys; print(json.load(sys.stdin)['id'])")
if [ -n "$WS_ID" ]; then
    pass "Workspace created: $WS_ID"
else
    fail "Could not create workspace"
fi

# Find the pane in the new workspace
sleep 2
PANES_JSON=$($CT pane list --json 2>&1)
PANE_COUNT=$(echo "$PANES_JSON" | python3 -c "import json,sys; print(len(json.load(sys.stdin)))")
echo "  Pane count: $PANE_COUNT"

# ─── Test 2: Set conductor via --pane ───────────────────────────
echo ""
echo "=== Test 2: Set conductor ==="
# Get the newest pane (likely in our workspace)
CONDUCTOR_ID=$(echo "$PANES_JSON" | python3 -c "
import json,sys
panes = json.load(sys.stdin)
# Find pane with no role
for p in panes:
    print(p['id'])
    break
")

RESULT=$($CT agent set-conductor --pane "$CONDUCTOR_ID" --json 2>&1)
if echo "$RESULT" | grep -q '"ok"'; then
    pass "Conductor set on $CONDUCTOR_ID"
else
    fail "Failed to set conductor: $RESULT"
fi

# Verify role
ROLE=$($CT agent status --pane "$CONDUCTOR_ID" --json 2>&1 | python3 -c "import json,sys; print(json.load(sys.stdin).get('role','none'))" 2>/dev/null)
if [ "$ROLE" = "Conductor" ]; then
    pass "Role verified: Conductor"
else
    fail "Expected role Conductor, got: $ROLE"
fi

# ─── Test 3: send-text executes (Enter is sent) ────────────────
echo ""
echo "=== Test 3: send-text adds Enter ==="
$CT send-text 'echo "SENDTEXT_OK" > /tmp/ct-eval-sendtext.txt' --pane "$CONDUCTOR_ID" 2>/dev/null
sleep 2
if [ -f /tmp/ct-eval-sendtext.txt ]; then
    CONTENT=$(cat /tmp/ct-eval-sendtext.txt)
    if [ "$CONTENT" = "SENDTEXT_OK" ]; then
        pass "send-text executes with Enter"
    else
        fail "send-text content mismatch: $CONTENT"
    fi
    rm -f /tmp/ct-eval-sendtext.txt
else
    fail "send-text did not execute (file not created)"
fi

# ─── Test 4: CODETERM_PANE_ID env var ───────────────────────────
echo ""
echo "=== Test 4: CODETERM_PANE_ID env var ==="
$CT send-text 'echo "$CODETERM_PANE_ID" > /tmp/ct-eval-paneid.txt' --pane "$CONDUCTOR_ID" 2>/dev/null
sleep 2
if [ -f /tmp/ct-eval-paneid.txt ]; then
    PANE_ID_FROM_ENV=$(cat /tmp/ct-eval-paneid.txt | tr -d '[:space:]')
    if [ "$PANE_ID_FROM_ENV" = "$CONDUCTOR_ID" ]; then
        pass "CODETERM_PANE_ID matches pane ID"
    else
        fail "CODETERM_PANE_ID='$PANE_ID_FROM_ENV' != '$CONDUCTOR_ID'"
    fi
    rm -f /tmp/ct-eval-paneid.txt
else
    fail "Could not read CODETERM_PANE_ID"
fi

# ─── Test 5: OSC 7 CWD tracking (invisible) ────────────────────
echo ""
echo "=== Test 5: CWD tracking ==="
# cd to a unique directory
$CT send-text 'cd /usr/local' --pane "$CONDUCTOR_ID" 2>/dev/null
sleep 3

CWD=$($CT pane list --json 2>&1 | python3 -c "
import json,sys
panes = json.load(sys.stdin)
for p in panes:
    if p['id'] == '$CONDUCTOR_ID':
        print(p.get('cwd',''))
")

# On macOS /usr/local might resolve to /private/usr/local or stay as /usr/local
if echo "$CWD" | grep -q "usr/local"; then
    pass "CWD tracked: $CWD"
else
    fail "CWD not tracked. Expected /usr/local, got: $CWD"
fi

# cd back
$CT send-text 'cd ~' --pane "$CONDUCTOR_ID" 2>/dev/null
sleep 1

# ─── Test 6: Terminal output clean (no OSC 7 injection visible) ─
echo ""
echo "=== Test 6: Clean terminal (no visible injection) ==="
OUTPUT=$($CT get-output --pane "$CONDUCTOR_ID" --lines 200 2>&1)
if echo "$OUTPUT" | grep -q "__ct_osc7"; then
    # Check if it's the command we sent (acceptable) or the injection (bad)
    if echo "$OUTPUT" | grep -q "if \[.*ZSH_VERSION"; then
        fail "OSC 7 injection script visible in terminal"
    else
        pass "No OSC 7 injection visible (hook name in output is from our test)"
    fi
else
    pass "No OSC 7 injection visible in terminal"
fi

# ─── Test 7: Spawn worker (own tab, linked in hierarchy) ────────
echo ""
echo "=== Test 7: Spawn worker (tab + hierarchy chain) ==="
# Count tabs before
TABS_BEFORE=$($CT workspace list --json 2>&1 | python3 -c "
import json,sys
ws = json.load(sys.stdin)
total = sum(w['tab_count'] for w in ws)
print(total)
")

# Spawn worker — creates own tab in conductor's workspace, linked in hierarchy
W1_RESULT=$($CT agent spawn bash --title "Worker-1" --role Worker --parent "$CONDUCTOR_ID" --cwd /tmp --task "echo worker1 ready" --json 2>&1)
W1_PANE=$(echo "$W1_RESULT" | python3 -c "import json,sys; d=json.load(sys.stdin); print(d.get('pane_id',''))" 2>/dev/null || echo "")

if [ -n "$W1_PANE" ]; then
    pass "Worker spawned: $W1_PANE"
else
    fail "Worker spawn failed: $W1_RESULT"
fi

sleep 3

# Count tabs after — should have +1 (each agent gets its own tab)
TABS_AFTER=$($CT workspace list --json 2>&1 | python3 -c "
import json,sys
ws = json.load(sys.stdin)
total = sum(w['tab_count'] for w in ws)
print(total)
")

if [ "$TABS_AFTER" -gt "$TABS_BEFORE" ]; then
    pass "Worker created as own tab ($TABS_BEFORE -> $TABS_AFTER)"
else
    fail "Tab count didn't increase ($TABS_BEFORE -> $TABS_AFTER)"
fi

# ─── Test 8: Hierarchy structure ────────────────────────────────
echo ""
echo "=== Test 8: Hierarchy ==="
HIERARCHY=$($CT agent hierarchy 2>&1)
echo "$HIERARCHY"

if echo "$HIERARCHY" | grep -q "Conductor"; then
    pass "Conductor visible in hierarchy"
else
    fail "No Conductor in hierarchy"
fi

if echo "$HIERARCHY" | grep -q "Worker-1"; then
    pass "Worker-1 visible in hierarchy"
else
    fail "Worker-1 not in hierarchy"
fi

# ─── Test 9: Children list ──────────────────────────────────────
echo ""
echo "=== Test 9: Children ==="
CHILDREN=$($CT agent children --pane "$CONDUCTOR_ID" 2>&1)
if echo "$CHILDREN" | grep -q "Worker-1"; then
    pass "Worker-1 listed as child of conductor"
else
    fail "Worker-1 not a child of conductor: $CHILDREN"
fi

# ─── Test 10: Multiple workers in same workspace ──────────────
echo ""
echo "=== Test 10: Second worker in conductor's workspace ==="
W2_RESULT=$($CT agent spawn bash --title "Worker-2" --role Worker --parent "$CONDUCTOR_ID" --json 2>&1)
W2_PANE=$(echo "$W2_RESULT" | python3 -c "import json,sys; d=json.load(sys.stdin); print(d.get('pane_id',''))" 2>/dev/null || echo "")

sleep 3

if [ -n "$W2_PANE" ]; then
    pass "Second worker spawned: $W2_PANE"
else
    fail "Second worker spawn failed"
fi

# ─── Test 11: 3-level depth (SubWorker) ─────────────────────────
echo ""
echo "=== Test 11: 3-level depth ==="
if [ -n "$W1_PANE" ]; then
    # Enable can_spawn on Worker-1
    $CT agent allow-spawn --pane "$W1_PANE" 2>/dev/null

    SW_RESULT=$($CT agent spawn bash --title "SubWorker-1" --role SubWorker --parent "$W1_PANE" --json 2>&1)
    SW_PANE=$(echo "$SW_RESULT" | python3 -c "import json,sys; d=json.load(sys.stdin); print(d.get('pane_id',''))" 2>/dev/null || echo "")

    if [ -n "$SW_PANE" ]; then
        pass "SubWorker spawned: $SW_PANE"

        sleep 2
        # Verify hierarchy shows 3 levels
        HIERARCHY3=$($CT agent hierarchy 2>&1)
        echo "$HIERARCHY3"

        if echo "$HIERARCHY3" | grep -q "SubWorker"; then
            pass "3-level hierarchy working"
        else
            fail "SubWorker not in hierarchy"
        fi
    else
        fail "SubWorker spawn failed: $SW_RESULT"
    fi
else
    skip "No Worker-1 pane for SubWorker test"
fi

# ─── Test 12: Depth violation (4th level) ───────────────────────
echo ""
echo "=== Test 12: Depth violation ==="
if [ -n "$SW_PANE" ]; then
    $CT agent allow-spawn --pane "$SW_PANE" 2>/dev/null
    DEEP_RESULT=$($CT agent spawn bash --title "TooDeep" --role SubWorker --parent "$SW_PANE" --json 2>&1)

    if echo "$DEEP_RESULT" | grep -qi "error\|depth\|max"; then
        pass "Depth violation correctly rejected"
    else
        # Check if it was created — might still work if validation is lenient
        DEEP_PANE=$(echo "$DEEP_RESULT" | python3 -c "import json,sys; d=json.load(sys.stdin); print(d.get('pane_id',''))" 2>/dev/null || echo "")
        if [ -n "$DEEP_PANE" ]; then
            fail "Depth violation NOT caught — 4th level pane created: $DEEP_PANE"
        else
            pass "Depth violation rejected (non-standard error format)"
        fi
    fi
else
    skip "No SubWorker pane for depth violation test"
fi

# ─── Test 13: Reports ───────────────────────────────────────────
echo ""
echo "=== Test 13: Reports ==="
if [ -n "$W1_PANE" ]; then
    $CT agent report "tests passed, all green" --pane "$W1_PANE" 2>/dev/null
    sleep 1
    REPORTS=$($CT agent reports --pane "$CONDUCTOR_ID" 2>&1)
    if echo "$REPORTS" | grep -q "tests passed"; then
        pass "Report received by conductor"
    else
        fail "Report not received: $REPORTS"
    fi
else
    skip "No Worker-1 for report test"
fi

# ─── Test 14: Invalid parent ───────────────────────────────────
echo ""
echo "=== Test 14: Invalid parent ==="
BAD_RESULT=$($CT agent spawn bash --title "Orphan" --role Worker --parent "nonexistent-pane-id" --json 2>&1)
if echo "$BAD_RESULT" | grep -qi "error\|not found"; then
    pass "Invalid parent correctly rejected"
else
    fail "Invalid parent not caught: $BAD_RESULT"
fi

# ─── Summary ───────────────────────────────────────────────────
echo ""
echo "========================================="
echo "  Results: $PASS passed, $FAIL failed, $SKIP skipped"
echo "========================================="

# Cleanup test files
rm -f /tmp/ct-eval-*.txt

exit $FAIL
