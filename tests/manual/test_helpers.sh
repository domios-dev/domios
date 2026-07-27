#!/usr/bin/env bash
# CodeTerm manual testing helpers — source this file, then use functions.
# Usage: source tests/manual/test_helpers.sh

set -euo pipefail

# Auto-detect dev instance
export CT_API="${CODETERM_API_URL:-http://127.0.0.1:7686}"
export CT_TOKEN="${CODETERM_API_TOKEN:-$(grep 'token' "$USERPROFILE/.codeterm-dev/config.toml" 2>/dev/null | sed 's/.*"\(.*\)".*/\1/' || echo '')}"

_ct() { curl -s "$CT_API$1" -H "Authorization: Bearer $CT_TOKEN" "${@:2}"; }
_ct_json() { _ct "$1" -H "Content-Type: application/json" "${@:2}"; }

# ── Tab helpers ─────────────────────────────────────────────

ct_tabs() {
  _ct /api/tabs | python -c "
import json,sys
for p in json.load(sys.stdin):
    at = str(p.get('agent_type',''))
    sid = str(p.get('agent_session_id',''))[:12]
    pp = '⚠PROMPT' if p.get('pending_prompt') else ''
    print(f'{p[\"id\"][:8]}  {p.get(\"title\",\"\"):<12}  agent={at or \"-\":<8}  session={sid or \"-\":<14}  {pp}')
"
}

ct_screen() {
  local tab="${1:-$(ct_first_tab)}"
  _ct "/api/tabs/$tab/text" | python -c "
import json,sys
s=json.load(sys.stdin).get('text','')
for i,l in enumerate(s.split(chr(10))):
    if l.strip(): print(f'{i:3}: {l.rstrip()[:100]}')
" 2>/dev/null
}

ct_first_tab() {
  _ct /api/tabs | python -c "import json,sys; print(json.load(sys.stdin)[0]['id'])"
}

ct_focus() {
  local tab="${1:-$(ct_first_tab)}"
  _ct_json "/api/tabs/$tab/focus" -X POST > /dev/null
  echo "Focused $tab"
}

ct_send() {
  local tab="$1"; shift
  local text="$*"
  _ct_json "/api/tabs/$tab/send" -X POST -d "{\"text\":\"$text\",\"enter\":true}" > /dev/null
}

ct_send_raw() {
  local tab="$1"; shift
  local text="$*"
  _ct_json "/api/tabs/$tab/send" -X POST \
    -d "{\"batch\":[{\"text\":\"$text\",\"enter\":false}]}" > /dev/null
}

ct_send_keys() {
  # Send a sequence of keys with delays: ct_send_keys TAB "\x1b" 200 "/exit" 150 "\r"
  local tab="$1"; shift
  local batch="["
  local first=true
  while [ $# -gt 0 ]; do
    local text="$1"; shift
    local delay="${1:-0}"; [ $# -gt 0 ] && shift
    if [ "$first" = true ]; then first=false; else batch+=","; fi
    batch+="{\"text\":\"$text\",\"enter\":false,\"delay_ms\":$delay}"
  done
  batch+="]"
  _ct_json "/api/tabs/$tab/send" -X POST -d "{\"batch\":$batch}" > /dev/null
}

# ── Agent helpers ────────────────────────────────────────────

ct_agent() {
  local tab="${1:-$(ct_first_tab)}"
  _ct /api/tabs | python -c "
import json,sys
p=next((x for x in json.load(sys.stdin) if x['id']=='$tab'),{})
at = p.get('agent_type','NONE')
sid = p.get('agent_session_id','none')
pp = 'yes' if p.get('pending_prompt') else 'no'
print(f'agent={at}  session={sid}  prompt={pp}')
"
}

ct_start_claude() {
  local tab="${1:-$(ct_first_tab)}"
  ct_focus "$tab"
  sleep 1
  ct_send "$tab" "claude"
  echo "Starting Claude in $tab..."
  # Poll for agent detection
  for i in $(seq 1 20); do
    sleep 3
    local at=$(_ct /api/tabs | python -c "import json,sys; p=next((x for x in json.load(sys.stdin) if x['id']=='$tab'),{}); print(p.get('agent_type','NONE'))" 2>/dev/null)
    if [ "$at" != "NONE" ]; then
      echo "✓ Agent detected: $at (${i}x3s)"
      ct_agent "$tab"
      return 0
    fi
  done
  echo "✗ Agent not detected after 60s"
  return 1
}

ct_hook() {
  local tab="${1:-$(ct_first_tab)}"
  local body="${2:-{}}"
  _ct_json "/api/tabs/$tab/hook/permission" -X POST -d "$body"
  echo ""
}

ct_hook_stop() {
  local tab="${1:-$(ct_first_tab)}"
  _ct_json "/api/tabs/$tab/hook/stop" -X POST
  echo ""
}

# ── Workspace helpers ────────────────────────────────────────

ct_workspaces() {
  _ct /api/workspaces | python -c "
import json,sys
for ws in json.load(sys.stdin):
    print(f'{ws[\"id\"]}  {ws.get(\"name\",\"\")}')
"
}

ct_new_tab() {
  local ws_id="${1:-$(_ct /api/workspaces | python -c "import json,sys; print(json.load(sys.stdin)[0]['id'])")}"
  local result=$(_ct_json "/api/workspaces/$ws_id/tabs" -X POST)
  local tab_id=$(echo "$result" | python -c "import json,sys; print(json.load(sys.stdin)['tab_id'])")
  echo "$tab_id"
}

# ── Quick status ─────────────────────────────────────────────

ct_status() {
  echo "=== Workspaces ==="
  ct_workspaces
  echo ""
  echo "=== Tabs ==="
  ct_tabs
}

ct_health() {
  _ct /api/health | python -c "import json,sys; d=json.load(sys.stdin); print(f'{d[\"name\"]} v{d[\"version\"]} — {d[\"status\"]}')"
}

# ── Logs ─────────────────────────────────────────────────────

ct_logs() {
  local n="${1:-20}"
  tail -"$n" "$USERPROFILE/.codeterm-dev/logs/codeterm.log" 2>/dev/null | grep -v "idle-poll\|NewEvents\|RedrawEvents"
}

ct_logs_agent() {
  grep -i "auto-detect\|agent exited\|ensure_agent\|\[hook\]\|\[resume\]" \
    "$USERPROFILE/.codeterm-dev/logs/codeterm.log" 2>/dev/null | tail -"${1:-20}"
}

ct_clear_logs() {
  echo "" > "$USERPROFILE/.codeterm-dev/logs/codeterm.log"
  echo "Logs cleared"
}

echo "CodeTerm test helpers loaded. API=$CT_API"
echo "Commands: ct_status ct_tabs ct_screen ct_agent ct_send ct_start_claude ct_hook ct_logs"
