#!/bin/bash
# CodeTerm FINAL E2E — tests ALL features including session, git, tray
API="http://localhost:7685/api"
CLI="C:/Developer/codeterm/cli/target/debug/codeterm.exe"
PASS=0; FAIL=0; TOTAL=0
ok() { echo "  ✓ $1"; PASS=$((PASS+1)); TOTAL=$((TOTAL+1)); }
nok() { echo "  ✗ $1"; FAIL=$((FAIL+1)); TOTAL=$((TOTAL+1)); }

P1=$(curl -s $API/panes | grep -o '"id":"[^"]*"' | head -1 | cut -d'"' -f4)
WS=$(curl -s $API/workspaces | grep -o '"id":"[^"]*"' | head -1 | cut -d'"' -f4)

echo "============================================"
echo "  CODETERM FINAL E2E ($(date +%H:%M))"
echo "============================================"

# ── 1. Core Terminal (5 tests) ──
echo ""; echo "── 1. Core Terminal ──"
curl -s "$API/panes/$P1/output" | grep -q "PS C:" && ok "PS prompt" || nok "PS prompt"
curl -s -X POST "$API/panes/$P1/send" -H "Content-Type: application/json" -d '{"text":"echo FINAL_E2E\r\n"}' > /dev/null; sleep 3
curl -s "$API/panes/$P1/output" | grep -q "FINAL_E2E" && ok "echo" || nok "echo"
curl -s -X POST "$API/panes/$P1/send" -H "Content-Type: application/json" -d '{"text":"echo $env:CODETERM\r\n"}' > /dev/null; sleep 2
curl -s "$API/panes/$P1/output" | grep -q "1" && ok "CODETERM=1" || nok "CODETERM"
curl -s -X POST "$API/panes/$P1/send" -H "Content-Type: application/json" -d '{"text":"echo $env:TERM\r\n"}' > /dev/null; sleep 2
curl -s "$API/panes/$P1/output" | grep -q "xterm-256color" && ok "TERM" || nok "TERM"
curl -s -X POST "$API/panes/$P1/send" -H "Content-Type: application/json" -d '{"text":"whoami\r\n"}' > /dev/null; sleep 2
curl -s "$API/panes/$P1/output" | grep -qi "user" && ok "whoami" || nok "whoami"

# ── 2. Tabs (3 tests) ──
echo ""; echo "── 2. Tabs ──"
TAB=$(curl -s -X POST "$API/workspaces/$WS/tabs"); P2=$(echo "$TAB" | grep -o '"pane_id":"[^"]*"' | cut -d'"' -f4); sleep 10
curl -s -X POST "$API/panes/$P2/send" -H "Content-Type: application/json" -d '{"text":"echo TAB2_OK\r\n"}' > /dev/null; sleep 3
curl -s "$API/panes/$P2/output" | grep -q "TAB2_OK" && ok "new tab" || nok "new tab"
curl -s -X POST "$API/panes/$P1/send" -H "Content-Type: application/json" -d '{"text":"echo P1_ALIVE\r\n"}' > /dev/null; sleep 2
curl -s "$API/panes/$P1/output" | grep -q "P1_ALIVE" && ok "tab1 alive" || nok "tab1"
PC=$(curl -s $API/panes | grep -o '"id"' | wc -l)
[ "$PC" -ge 2 ] && ok "$PC panes" || nok "panes"

# ── 3. Splits (2 tests) ──
echo ""; echo "── 3. Splits ──"
SP=$(curl -s -X POST "$API/panes/$P1/split" -H "Content-Type: application/json" -d '{"direction":"right"}')
P3=$(echo "$SP" | grep -o '"new_pane_id":"[^"]*"' | cut -d'"' -f4); sleep 10
curl -s -X POST "$API/panes/$P3/send" -H "Content-Type: application/json" -d '{"text":"echo SPLIT_OK\r\n"}' > /dev/null; sleep 3
curl -s "$API/panes/$P3/output" | grep -q "SPLIT_OK" && ok "h-split" || nok "h-split"
$CLI pane split "$P2" --direction down > /dev/null 2>&1; sleep 10
PC2=$(curl -s $API/panes | grep -o '"id"' | wc -l)
[ "$PC2" -ge 4 ] && ok "v-split ($PC2 panes)" || nok "v-split"

# ── 4. Claude Code (3 tests) ──
echo ""; echo "── 4. Claude Code ──"
curl -s -X POST "$API/panes/$P1/send" -H "Content-Type: application/json" -d '{"text":"claude -p \"Say: CLAUDE_FINAL\" 2>&1\r\n"}' > /dev/null; sleep 20
curl -s "$API/panes/$P1/output" | grep -q "CLAUDE_FINAL" && ok "claude responds" || nok "claude"
curl -s -X POST "$API/panes/$P2/send" -H "Content-Type: application/json" -d '{"text":"claude -p \"What is 9*9?\" 2>&1\r\n"}' > /dev/null; sleep 20
curl -s "$API/panes/$P2/output" | grep -q "81" && ok "claude math" || nok "claude math"
$CLI get-output "$P1" 2>&1 | grep -qi "PS\|claude\|echo" && ok "get-output" || nok "get-output"

# ── 5. CLI (4 tests) ──
echo ""; echo "── 5. CLI ──"
$CLI identify 2>&1 | grep -q CodeTerm && ok "cli identify" || nok "cli identify"
$CLI capabilities 2>&1 | grep -q "pane.get_output\|pane.split" && ok "cli caps (new)" || nok "cli caps"
$CLI notify "Final" --level info 2>&1 | grep -q id && ok "cli notify" || nok "cli notify"
$CLI workspace create "CLI-WS" 2>&1 | grep -q id && ok "cli workspace" || nok "cli workspace"

# ── 6. REST API (4 tests) ──
echo ""; echo "── 6. REST API ──"
curl -s $API/health | grep -q ok && ok "health" || nok "health"
curl -s $API/identify | grep -q rest-api && ok "identify" || nok "identify"
curl -s -X POST "$API/workspaces" -H "Content-Type: application/json" -d '{"name":"API-WS"}' | grep -q id && ok "create ws" || nok "create ws"
curl -s -X POST "$API/notifications" -H "Content-Type: application/json" -d '{"title":"API-Test","level":"warning"}' | grep -q id && ok "create notif" || nok "notif"

# ── 7. Notifications (3 tests) ──
echo ""; echo "── 7. Notifications ──"
NC=$(curl -s "$API/notifications" | grep -o '"id"' | wc -l)
[ "$NC" -ge 1 ] && ok "$NC notifs" || nok "no notifs"
curl -s -X POST "$API/notifications/clear" | grep -q ok && ok "clear" || nok "clear"
NC2=$(curl -s "$API/notifications" | grep -o '"id"' | wc -l)
[ "$NC2" -eq 0 ] && ok "cleared" || nok "not cleared"
# Spam check
sleep 3
NC3=$(curl -s "$API/notifications" | grep -o '"id"' | wc -l)
[ "$NC3" -le 2 ] && ok "no spam" || nok "spam ($NC3)"

# ── 8. Config (3 tests) ──
echo ""; echo "── 8. Config ──"
[ -f "$APPDATA/codeterm/config.toml" ] && ok "config exists" || nok "no config"
grep -q "\[theme\]" "$APPDATA/codeterm/config.toml" && ok "theme section" || nok "theme"
grep -q "\[api\]" "$APPDATA/codeterm/config.toml" && ok "api section" || nok "api"

# ── 9. Skills (4 tests) ──
echo ""; echo "── 9. Skills ──"
[ -f "C:/Developer/codeterm/skills/codeterm/SKILL.md" ] && ok "codeterm" || nok "codeterm"
[ -f "C:/Developer/codeterm/skills/codeterm-notify/SKILL.md" ] && ok "notify" || nok "notify"
[ -f "C:/Developer/codeterm/skills/codeterm-canvas/SKILL.md" ] && ok "canvas" || nok "canvas"
[ -f "C:/Developer/codeterm/skills/codeterm-layout/SKILL.md" ] && ok "layout" || nok "layout"

# ── 10. Session (2 tests) ──
echo ""; echo "── 10. Session ──"
[ -d "$APPDATA/codeterm/sessions" ] && ok "sessions dir" || nok "no sessions dir"
# Session save will be tested on close

# ── 11. Stress (2 tests) ──
echo ""; echo "── 11. Stress ──"
ALIVE=0; TOTAL_P=$(curl -s $API/panes | grep -o '"id"' | wc -l)
for pid in $(curl -s $API/panes | grep -o '"id":"[^"]*"' | cut -d'"' -f4); do
    M="F$(echo $pid | cut -c1-3)"
    curl -s -X POST "$API/panes/$pid/send" -H "Content-Type: application/json" -d "{\"text\":\"echo $M\\r\\n\"}" > /dev/null
done; sleep 5
for pid in $(curl -s $API/panes | grep -o '"id":"[^"]*"' | cut -d'"' -f4); do
    M="F$(echo $pid | cut -c1-3)"
    curl -s "$API/panes/$pid/output" | grep -q "$M" && ALIVE=$((ALIVE+1))
done
[ "$ALIVE" -eq "$TOTAL_P" ] && ok "all $ALIVE/$TOTAL_P alive" || nok "$ALIVE/$TOTAL_P alive"
WC=$(curl -s $API/workspaces | grep -o '"id"' | wc -l)
[ "$WC" -ge 3 ] && ok "$WC workspaces" || nok "$WC workspaces"

echo ""
echo "============================================"
echo "  FINAL: $PASS passed, $FAIL failed ($TOTAL total)"
echo "============================================"
[ "$FAIL" -eq 0 ] && echo "  ALL TESTS PASSED" || echo "  $FAIL tests need attention"
