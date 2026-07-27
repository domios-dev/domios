#!/usr/bin/env bash
# Seed the Canvas v2 dogfood plan on dev (7686). Reusable until the daemon
# reliably re-scans .codeterm/plans/ on restart. Prints PLAN_ID.
set -uo pipefail
PROJ="${1:-/Users/rollacode/Developer/codeterm}"
TOKEN=$(grep -E '^token = ' ~/.codeterm-dev/config.toml | head -1 | sed 's/.*"\(.*\)"/\1/')
B="http://127.0.0.1:7686"; A=(-H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json")
post(){ curl -s "${A[@]}" -X POST "$B$1" -d "$2"; }
patch(){ curl -s "${A[@]}" -X PATCH "$B$1" -d "$2" >/dev/null; }

PID=$(post /api/plans "{\"title\":\"Canvas v2 — Figma-grade plan board (dogfood)\",\"project_path\":\"$PROJ\",\"kinds\":\"status_update,decision,testing,review\"}" | jq -r '.plan_id // .id')
mksec(){ post /api/plans/$PID/sections "{\"title\":\"$1\",\"body\":\"$2\"}" | jq -r '.section_id // .id'; }
S1=$(mksec "Layout redesign" "Compact sectioned plan node + separate spec node + satellite task nodes joined by curved edges to section-row anchors + milestone (Step) bubble clusters grouped by kind + orchestrator groups. Covered sections fill-tinted; uncovered pop. Replaces the v1 frame-with-bands model.")
S2=$(mksec "Navigation" "Figma-grade trackpad: two-finger scroll pans both axes; Cmd/Ctrl+scroll zooms around pointer; pinch zoom; middle+space drag; smooth RAF; non-passive wheel. No manual node dragging — deterministic auto-pack.")
S3=$(mksec "Workspace isolation" "Canvas shows ONLY plans whose project_path == this workspace's git toplevel. One workspace = one .codeterm/plans. Daemon must re-scan plans on restart (persisted plans survive).")
S4=$(mksec "Standalone tasks (Inbox)" "Tasks may exist without a plan — reserved per-project Inbox plan (id inbox), reusing all plan machinery. Takeable by worker OR orchestrator; link chip opens that tab.")
S5=$(mksec "Graph / Task CLI + canvas focus" "codeterm task list/show/take/drop/add, plan gaps, codeterm canvas focus (camera+blink) over emit_everywhere.")
S6=$(mksec "Plan-as-mode + composer + activity bubble" "Plan as third tab mode (Chat|Terminal|Plan), highlighted; returns via Chat/Terminal. Composer clears on send. Non-modal activity bubble shows the addressed orchestrator's live status.")

T(){ local n=$1 ty=$2 mo=$3 wy=$4 bo=$5 secs=$6 st=$7; shift 7
  local tid; tid=$(post /api/plans/$PID/tasks "{\"name\":\"$n\",\"type\":\"$ty\",\"model\":\"$mo\",\"why\":\"$wy\",\"body\":\"$bo\",\"sections\":\"$secs\"}" | jq -r '.task_id // .id')
  for m in "$@"; do post /api/plans/$PID/tasks/$tid/milestones "{\"kind\":\"${m%%:*}\",\"text\":\"${m#*:}\"}" >/dev/null; done
  [ "$st" = in_progress ] && patch /api/plans/$PID/tasks/$tid '{"status":"in_progress"}'
  [ "$st" = done ] && patch /api/plans/$PID/tasks/$tid '{"status":"done"}'
}
T "v2-FE redesign" implement "claude:opus" "design-critical: frontend-design skill + layout/nav reasoning" \
  "Sectioned plan node, spec node, curved section-row->task edges, Step bubble clusters, orchestrator groups, trackpad nav, plan-as-mode, composer clear+activity bubble. **Markdown** supported." \
  "$S1,$S2,$S6" done \
  "decision:Root cause — v1 frame-with-bands layout produced scattered edge-less board; replaced with compact sectioned node + connected satellites" \
  "review:Structural cut landed; deleted PlanFrame/SectionCard/useCanvasNodeDrag (no-drag)" \
  "testing:tsc clean, 12/12 canvas tsx tests green"
T "v2-BE tasks/CLI" implement "codex:gpt-5.5" "mechanical backend wiring over existing JSON store; faster" \
  "Standalone tasks via Inbox plan, codeterm task CLI + canvas focus emit, workspace-scoping, plan gaps, archive cascade." \
  "$S3,$S4,$S5" done \
  "decision:Reuse plan machinery (Inbox plan) for standalone tasks — no new store" \
  "testing:cargo plan 43 + task 35 green; manifest fresh"
echo "PLAN_ID=$PID"
