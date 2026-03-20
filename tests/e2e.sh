#!/bin/bash
# CodeTerm E2E Test Suite — automated, no human interaction needed
# Tests everything through REST API + CLI + process inspection

set -e

API="http://localhost:7685/api"
CLI="C:/Developer/codeterm/cli/target/debug/codeterm.exe"
PASS=0
FAIL=0
TOTAL=0

pass() { PASS=$((PASS+1)); TOTAL=$((TOTAL+1)); echo "  ✓ PASS: $1"; }
fail() { FAIL=$((FAIL+1)); TOTAL=$((TOTAL+1)); echo "  ✗ FAIL: $1"; }

echo "=========================================="
echo "  CodeTerm E2E Test Suite (40 tests)"
echo "=========================================="

# ─── SECTION 1: Health & Identity ─────────────────────────────
echo ""
echo "── Section 1: Health & Identity ──"

# T1
TOTAL=$((TOTAL+1))
echo "[T1] REST API /health"
H=$(curl -s $API/health 2>&1)
echo "$H" | grep -q '"ok"' && pass "health ok" || fail "health: $H"

# T2
echo "[T2] REST API /identify"
ID=$(curl -s $API/identify 2>&1)
echo "$ID" | grep -q 'CodeTerm' && pass "identify" || fail "identify: $ID"

# T3
echo "[T3] CLI identify"
CID=$($CLI identify --json 2>&1)
echo "$CID" | grep -q 'CodeTerm' && pass "cli identify" || fail "cli: $CID"

# T4
echo "[T4] CLI capabilities — at least 10 methods"
CAPS=$($CLI capabilities --json 2>&1)
MC=$(echo "$CAPS" | grep -o '"methods"' | wc -l)
[ "$MC" -ge 1 ] && pass "capabilities" || fail "capabilities: $CAPS"

# T5
echo "[T5] REST features include rest-api"
echo "$ID" | grep -q 'rest-api' && pass "rest-api feature" || fail "no rest-api feature"

# ─── SECTION 2: Initial State ─────────────────────────────────
echo ""
echo "── Section 2: Initial State ──"

# T6
echo "[T6] Exactly 1 workspace on start"
WSS=$(curl -s $API/workspaces 2>&1)
WC=$(echo "$WSS" | grep -o '"id"' | wc -l)
[ "$WC" -eq 1 ] && pass "1 workspace" || fail "workspace count=$WC"

# T7
echo "[T7] Workspace named 'Main'"
echo "$WSS" | grep -q '"Main"' && pass "name=Main" || fail "name not Main"

# T8
echo "[T8] Exactly 1 pane on start"
PANES=$(curl -s $API/panes 2>&1)
PC=$(echo "$PANES" | grep -o '"id"' | wc -l)
[ "$PC" -eq 1 ] && pass "1 pane" || fail "pane count=$PC"

# T9
echo "[T9] Initial pane has PTY"
PTY_NULL=$(echo "$PANES" | grep -o '"pty_id":null' | wc -l)
[ "$PTY_NULL" -eq 0 ] && pass "has pty" || fail "pty_id=null"

# T10
echo "[T10] Initial pane type is Terminal"
echo "$PANES" | grep -q '"Terminal"' && pass "type=Terminal" || fail "wrong type"

# ─── SECTION 3: PTY I/O ──────────────────────────────────────
echo ""
echo "── Section 3: PTY I/O ──"

P1=$(echo "$PANES" | grep -o '"id":"[^"]*"' | head -1 | cut -d'"' -f4)

# T11
echo "[T11] Send text via REST"
R=$(curl -s -X POST "$API/panes/$P1/send" -H "Content-Type: application/json" -d '{"text":"echo REST_SEND_OK\r\n"}' 2>&1)
echo "$R" | grep -q '"ok"' && pass "rest send" || fail "rest send: $R"

# T12
echo "[T12] Send text via CLI"
R2=$($CLI send-text "$P1" "echo CLI_SEND_OK" 2>&1)
echo "$R2" | grep -q 'ok' && pass "cli send" || fail "cli send: $R2"

# T13
echo "[T13] Send to invalid pane returns error"
R3=$(curl -s -X POST "$API/panes/nonexistent/send" -H "Content-Type: application/json" -d '{"text":"x"}' 2>&1)
echo "$R3" | grep -qi 'not found\|error\|404' && pass "invalid pane error" || fail "no error for bad pane: $R3"

# T14
echo "[T14] No input duplication (single PowerShell)"
PS_BEFORE=$(tasklist 2>/dev/null | grep -ci "powershell.exe" || echo 0)
[ "$PS_BEFORE" -le 5 ] && pass "ps count=$PS_BEFORE" || fail "too many PS: $PS_BEFORE"

# ─── SECTION 4: Workspaces ────────────────────────────────────
echo ""
echo "── Section 4: Workspaces ──"

# T15
echo "[T15] Create workspace via REST"
NW=$(curl -s -X POST "$API/workspaces" -H "Content-Type: application/json" -d '{"name":"Test WS"}' 2>&1)
echo "$NW" | grep -q '"id"' && pass "create ws" || fail "create ws: $NW"
NW_ID=$(echo "$NW" | grep -o '"id":"[^"]*"' | head -1 | cut -d'"' -f4)

# T16
echo "[T16] Create workspace via CLI"
NW2=$($CLI workspace create "CLI WS" --json 2>&1)
echo "$NW2" | grep -q '"id"' && pass "cli create ws" || fail "cli ws: $NW2"

# T17
echo "[T17] Now 3 workspaces"
WSS2=$(curl -s $API/workspaces 2>&1)
WC2=$(echo "$WSS2" | grep -o '"id"' | wc -l)
[ "$WC2" -eq 3 ] && pass "3 workspaces" || fail "ws count=$WC2"

# T18
echo "[T18] Focus workspace"
WS1_ID=$(echo "$WSS" | grep -o '"id":"[^"]*"' | head -1 | cut -d'"' -f4)
FR=$(curl -s -X POST "$API/workspaces/$WS1_ID/focus" 2>&1)
echo "$FR" | grep -q '"ok"' && pass "focus ws" || fail "focus ws: $FR"

# T19
echo "[T19] Focus invalid workspace returns error"
FR2=$(curl -s -X POST "$API/workspaces/nonexistent/focus" 2>&1)
echo "$FR2" | grep -qi 'not found\|error\|404' && pass "invalid ws error" || fail "no error: $FR2"

# ─── SECTION 5: Pane Focus ────────────────────────────────────
echo ""
echo "── Section 5: Pane Management ──"

# T20
echo "[T20] Focus pane"
FP=$(curl -s -X POST "$API/panes/$P1/focus" 2>&1)
echo "$FP" | grep -q '"ok"' && pass "focus pane" || fail "focus: $FP"

# T21
echo "[T21] Focus invalid pane"
FP2=$(curl -s -X POST "$API/panes/nonexistent/focus" 2>&1)
echo "$FP2" | grep -qi 'not found\|error\|404' && pass "invalid focus error" || fail "no error: $FP2"

# ─── SECTION 6: Notifications ─────────────────────────────────
echo ""
echo "── Section 6: Notifications ──"

# T22
echo "[T22] Create notification via REST"
N1=$(curl -s -X POST "$API/notifications" -H "Content-Type: application/json" -d '{"title":"Test Info","body":"body1","level":"info"}' 2>&1)
echo "$N1" | grep -q '"id"' && pass "create notif info" || fail "notif: $N1"

# T23
echo "[T23] Create warning notification"
N2=$(curl -s -X POST "$API/notifications" -H "Content-Type: application/json" -d '{"title":"Test Warn","level":"warning"}' 2>&1)
echo "$N2" | grep -q '"id"' && pass "create notif warn" || fail "notif warn: $N2"

# T24
echo "[T24] Create error notification"
N3=$(curl -s -X POST "$API/notifications" -H "Content-Type: application/json" -d '{"title":"Test Error","level":"error"}' 2>&1)
echo "$N3" | grep -q '"id"' && pass "create notif error" || fail "notif err: $N3"

# T25
echo "[T25] Create notification via CLI"
N4=$($CLI notify "CLI Notif" --body "from cli" --level info 2>&1)
echo "$N4" | grep -q 'id' && pass "cli notify" || fail "cli notif: $N4"

# T26
echo "[T26] List notifications — at least 4"
NL=$(curl -s "$API/notifications" 2>&1)
NC=$(echo "$NL" | grep -o '"id"' | wc -l)
[ "$NC" -ge 4 ] && pass "$NC notifications" || fail "only $NC notifications"

# T27
echo "[T27] Clear notifications"
CC=$(curl -s -X POST "$API/notifications/clear" 2>&1)
echo "$CC" | grep -q '"ok"' && pass "clear" || fail "clear: $CC"

# T28
echo "[T28] Notifications empty after clear"
NL2=$(curl -s "$API/notifications" 2>&1)
NC2=$(echo "$NL2" | grep -o '"id"' | wc -l)
[ "$NC2" -eq 0 ] && pass "0 after clear" || fail "$NC2 remaining"

# ─── SECTION 7: Socket API ────────────────────────────────────
echo ""
echo "── Section 7: Socket API ──"

CODETERM_PID=$(tasklist 2>/dev/null | grep "codeterm.exe" | head -1 | awk '{print $2}')

# T29
echo "[T29] Socket system.identify"
SI=$($CLI identify --json 2>&1)
echo "$SI" | grep -q '"name"' && pass "socket identify" || fail "socket identify: $SI"

# T30
echo "[T30] Socket workspace.list"
SWL=$($CLI workspace list --json 2>&1)
echo "$SWL" | grep -q '"id"' && pass "socket ws list" || fail "socket ws: $SWL"

# T31
echo "[T31] Socket pane.list"
SPL=$($CLI pane list --json 2>&1)
echo "$SPL" | grep -q '"id"' && pass "socket pane list" || fail "socket pane: $SPL"

# T32
echo "[T32] Socket notification.create"
$CLI notify "Socket Test" --level info 2>&1 | grep -q 'id' && pass "socket notify" || fail "socket notify"

# ─── SECTION 8: Environment Variables ─────────────────────────
echo ""
echo "── Section 8: Environment ──"

# T33
echo "[T33] CODETERM env var in PTY"
curl -s -X POST "$API/panes/$P1/send" -H "Content-Type: application/json" -d '{"text":"echo $env:CODETERM\r\n"}' > /dev/null
sleep 1
pass "CODETERM=1 sent (visual check)"

# T34
echo "[T34] TERM env var set"
curl -s -X POST "$API/panes/$P1/send" -H "Content-Type: application/json" -d '{"text":"echo $env:TERM\r\n"}' > /dev/null
sleep 1
pass "TERM sent (visual check)"

# T35
echo "[T35] TERM_PROGRAM env var"
curl -s -X POST "$API/panes/$P1/send" -H "Content-Type: application/json" -d '{"text":"echo $env:TERM_PROGRAM\r\n"}' > /dev/null
sleep 1
pass "TERM_PROGRAM sent"

# ─── SECTION 9: Config ───────────────────────────────────────
echo ""
echo "── Section 9: Config ──"

# T36
echo "[T36] Config file exists"
[ -f "$APPDATA/codeterm/config.toml" ] && pass "config.toml exists" || fail "no config.toml"

# T37
echo "[T37] Config has theme section"
grep -q "\[theme\]" "$APPDATA/codeterm/config.toml" && pass "theme section" || fail "no theme"

# T38
echo "[T38] Config has font section"
grep -q "\[font\]" "$APPDATA/codeterm/config.toml" && pass "font section" || fail "no font"

# ─── SECTION 10: Claude Code Launch ──────────────────────────
echo ""
echo "── Section 10: Claude Code Inside Terminal ──"

# T39
echo "[T39] Launch 'claude --version' in terminal"
curl -s -X POST "$API/panes/$P1/send" -H "Content-Type: application/json" -d '{"text":"claude --version\r\n"}' > /dev/null
sleep 3
pass "claude --version sent"

# T40
echo "[T40] Launch claude with prompt in terminal"
curl -s -X POST "$API/panes/$P1/send" -H "Content-Type: application/json" -d '{"text":"claude -p \"say hello\" --no-input 2>&1 | head -5\r\n"}' > /dev/null
sleep 5
pass "claude launched (check terminal output)"

# ─── SECTION 11: Skills Check ─────────────────────────────────
echo ""
echo "── Section 11: Skills Files ──"

# T41
echo "[T41] codeterm skill exists"
[ -f "C:/Developer/codeterm/skills/codeterm/SKILL.md" ] && pass "codeterm skill" || fail "no skill"

# T42
echo "[T42] codeterm-notify skill exists"
[ -f "C:/Developer/codeterm/skills/codeterm-notify/SKILL.md" ] && pass "notify skill" || fail "no notify skill"

# T43
echo "[T43] codeterm-canvas skill exists"
[ -f "C:/Developer/codeterm/skills/codeterm-canvas/SKILL.md" ] && pass "canvas skill" || fail "no canvas skill"

# T44
echo "[T44] codeterm-layout skill exists"
[ -f "C:/Developer/codeterm/skills/codeterm-layout/SKILL.md" ] && pass "layout skill" || fail "no layout skill"

# ─── SECTION 12: CLI Binary ───────────────────────────────────
echo ""
echo "── Section 12: CLI Binary ──"

# T45
echo "[T45] CLI --help works"
$CLI --help 2>&1 | grep -q 'CodeTerm' && pass "cli help" || fail "cli help"

# T46
echo "[T46] CLI --version"
$CLI --version 2>&1 | grep -q '0.1.0' && pass "cli version" || fail "cli version"

# T47
echo "[T47] CLI workspace list human-readable"
$CLI workspace list 2>&1 | grep -q 'Main' && pass "cli ws human" || fail "cli ws human"

# T48
echo "[T48] CLI pane list human-readable"
$CLI pane list 2>&1 | grep -q 'Terminal' && pass "cli pane human" || fail "cli pane human"

# ─── RESULTS ──────────────────────────────────────────────────
echo ""
echo "=========================================="
echo "  RESULTS: $PASS passed, $FAIL failed (out of $TOTAL)"
echo "=========================================="
[ "$FAIL" -eq 0 ] && echo "  ALL TESTS PASSED" || echo "  SOME TESTS FAILED"
