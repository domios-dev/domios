#!/bin/bash
# Agent Scenario Test — simulates an external agent (like Rollo/OpenClaw)
# connecting to CodeTerm via REST API to orchestrate work

API="http://localhost:7685/api"
CLI="C:/Developer/codeterm/cli/target/debug/codeterm.exe"
PASS=0; FAIL=0
ok() { echo "  ✓ $1"; PASS=$((PASS+1)); }
nok() { echo "  ✗ $1"; FAIL=$((FAIL+1)); }

echo "============================================"
echo "  AGENT SCENARIO: External orchestrator"
echo "  Simulates Rollo/OpenClaw connecting to"
echo "  CodeTerm and managing agent sessions"
echo "============================================"

# 1. Agent discovers CodeTerm
echo ""; echo "── Phase 1: Discovery ──"
ID=$(curl -s $API/identify 2>&1)
echo "$ID" | grep -q "CodeTerm" && ok "Discovered CodeTerm" || { nok "No CodeTerm"; exit 1; }
echo "  Version: $(echo $ID | grep -o '"version":"[^"]*"' | cut -d'"' -f4)"

# 2. Agent checks capabilities
CAPS=$(curl -s $API/identify 2>&1)
echo "$CAPS" | grep -q "rest-api" && ok "REST API available" || nok "No REST API"

# 3. Agent creates a dedicated workspace
echo ""; echo "── Phase 2: Setup workspace ──"
WS=$(curl -s -X POST "$API/workspaces" -H "Content-Type: application/json" -d '{"name":"Agent-Rollo"}')
WS_ID=$(echo "$WS" | grep -o '"id":"[^"]*"' | head -1 | cut -d'"' -f4)
[ -n "$WS_ID" ] && ok "Created workspace: Agent-Rollo" || nok "Failed to create workspace"

# 4. Agent gets existing pane (initial workspace tab)
P1=$(curl -s $API/panes | grep -o '"id":"[^"]*"' | head -1 | cut -d'"' -f4)
[ -n "$P1" ] && ok "Found initial pane: ${P1:0:8}" || nok "No panes"

# 5. Agent sends notification
echo ""; echo "── Phase 3: Communication ──"
curl -s -X POST "$API/notifications" -H "Content-Type: application/json" \
    -d '{"title":"Agent Connected","body":"Rollo connected to CodeTerm","level":"info"}' | grep -q '"id"' \
    && ok "Sent connection notification" || nok "Notification failed"

# 6. Agent sends command to terminal
echo ""; echo "── Phase 4: Command execution ──"
curl -s -X POST "$API/panes/$P1/send" -H "Content-Type: application/json" \
    -d '{"text":"echo AGENT_CONNECTED_$(date +%s)\r\n"}' > /dev/null
sleep 3
OUT=$(curl -s "$API/panes/$P1/output" 2>&1)
echo "$OUT" | grep -q "AGENT_CONNECTED" && ok "Command executed in terminal" || nok "Command failed"

# 7. Agent reads terminal output to verify
echo "$OUT" | grep -q "PS C:" && ok "Can read terminal state" || nok "Can't read state"

# 8. Agent launches Claude in the terminal
echo ""; echo "── Phase 5: Launch Claude ──"
curl -s -X POST "$API/panes/$P1/send" -H "Content-Type: application/json" \
    -d '{"text":"claude -p \"Respond: AGENT_CLAUDE_READY\" 2>&1\r\n"}' > /dev/null
echo "  Waiting for Claude (20s)..."
sleep 20
OUT2=$(curl -s "$API/panes/$P1/output" 2>&1)
echo "$OUT2" | grep -q "AGENT_CLAUDE_READY" && ok "Claude responded to agent" || nok "Claude no response"

# 9. Agent creates new tab for background task
echo ""; echo "── Phase 6: Multi-tab operations ──"
WS1=$(curl -s $API/workspaces | grep -o '"id":"[^"]*"' | head -1 | cut -d'"' -f4)
TAB=$(curl -s -X POST "$API/workspaces/$WS1/tabs")
P2=$(echo "$TAB" | grep -o '"pane_id":"[^"]*"' | cut -d'"' -f4)
sleep 10
curl -s -X POST "$API/panes/$P2/send" -H "Content-Type: application/json" \
    -d '{"text":"echo BACKGROUND_TASK_RUNNING\r\n"}' > /dev/null
sleep 3
curl -s "$API/panes/$P2/output" | grep -q "BACKGROUND_TASK" && ok "Background tab works" || nok "Background tab failed"

# 10. Agent verifies both panes work simultaneously
curl -s -X POST "$API/panes/$P1/send" -H "Content-Type: application/json" -d '{"text":"echo P1_STILL_OK\r\n"}' > /dev/null
sleep 2
curl -s "$API/panes/$P1/output" | grep -q "P1_STILL_OK" && ok "Main pane still alive" || nok "Main pane dead"

# 11. Agent splits pane for monitoring
echo ""; echo "── Phase 7: Split for monitoring ──"
SPLIT=$(curl -s -X POST "$API/panes/$P1/split" -H "Content-Type: application/json" -d '{"direction":"right"}')
P3=$(echo "$SPLIT" | grep -o '"new_pane_id":"[^"]*"' | cut -d'"' -f4)
sleep 10
curl -s -X POST "$API/panes/$P3/send" -H "Content-Type: application/json" \
    -d '{"text":"echo MONITOR_PANE\r\n"}' > /dev/null
sleep 3
curl -s "$API/panes/$P3/output" | grep -q "MONITOR_PANE" && ok "Monitor split works" || nok "Split failed"

# 12. Agent sends completion notification
echo ""; echo "── Phase 8: Cleanup ──"
curl -s -X POST "$API/notifications" -H "Content-Type: application/json" \
    -d '{"title":"Agent Complete","body":"All tasks finished successfully","level":"info"}' | grep -q '"id"' \
    && ok "Completion notification sent" || nok "Notification failed"

# 13. Final state check
echo ""; echo "── Final State ──"
PC=$(curl -s $API/panes | grep -o '"id"' | wc -l)
WC=$(curl -s $API/workspaces | grep -o '"id"' | wc -l)
NC=$(curl -s "$API/notifications" | grep -o '"id"' | wc -l)
echo "  Panes: $PC | Workspaces: $WC | Notifications: $NC"
curl -s $API/health | grep -q "ok" && ok "App alive" || nok "App dead"

echo ""
echo "============================================"
echo "  RESULTS: $PASS passed, $FAIL failed"
echo "============================================"
