#!/usr/bin/env bash
# §32 WORLDS v2 dogfood seed for the ISOLATED worlds2 dev instance (port 7696).
# Rich data for the §32.9 matrix: varied statuses (active/done/pending/blocked) +
# priorities (cloud sort / done-sinks), strides of several kinds (snake), subtasks,
# and handoffs (timeline lane). Scoped to the dev's active workspace so the board
# shows it. Targets 7696 only — never the canonical dev.
set -uo pipefail
PORT=7696 # force the isolated instance (parent tabs leak CODETERM_API_PORT=7685)
TOKEN=$(grep -E '^token = ' ~/.codeterm-dev-worlds2/config.toml | head -1 | sed 's/.*"\(.*\)"/\1/')
B="http://127.0.0.1:$PORT"; A=(-H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json")
jpost(){ curl -s "${A[@]}" -X POST "$B$1" -d "$2"; }
jpatch(){ curl -s "${A[@]}" -X PATCH "$B$1" -d "$2" >/dev/null; }

# Use the active workspace's cwd so the workspace board shows these plans.
PROJ=$(curl -s "${A[@]}" "$B/api/workspaces" | jq -r 'if type=="array" then . else .workspaces end | map(select(.is_active==true))[0].cwd // empty' | head -1)
[ -z "$PROJ" ] && PROJ="/Users/rollacode/Developer/codeterm-worlds2"
echo "seeding project_path=$PROJ on $B"

sec(){ jq -n --arg t "$1" --arg b "$2" '{title:$t,body:$b}' ; }
addsec(){ jpost /api/plans/$PID/sections "$(sec "$1" "$2")" | jq -r '.section_id // .id'; }

PID=$(jpost /api/plans "$(jq -n --arg p "$PROJ" '{title:"WORLDS v2 — live board dogfood",project_path:$p,kinds:"status_update,decision,testing,review,handoff"}')" | jq -r '.plan_id // .id')
echo "plan=$PID"

S1=$(addsec "Spine & channels" "Four ORTHOGONAL channels, never overloaded: SPATIAL-Y = hierarchy (in_progress rises, done sinks), PAINT band (W0-W3) = detail, COLOR = identity, OPACITY = transient focus only. Primary content text is always opacity 1.0 — status is encoded by POSITION, not fade.")
S2=$(addsec "Birds-eye W0" "W0 is COMPRESSED content, not less content. A plan shows title, owner, coverage bar, top section rows + status counts. A task shows name, a solid status chip, a 2-line digest, body/stride/subtask counters, a progress mini-bar, and a stride-snake preview. Because size tracks content, a node is never a big hollow box.")
S3=$(addsec "Stride snake" "Strides render as ONE continuous 2px path threading kind-colored circular beads (Thread & Coil), wrapping as a boustrophedon snake. Freshest bead pulses at the head; done strides coil to the tail. Zoom blooms beads into full stride cards while the path stays as a provenance spine. Each stride is a leaf micro-world.")
S4=$(addsec "Task cloud" "Tasks pack as a 2D CLOUD to the right of the plan, not a tall column. The vertical axis encodes status/priority: high-priority in_progress at the top, done sinking to the bottom. Overflow flows rightward into more columns, never an infinite vertical strip. A status change animates the task to its new slot (~300ms), no global reshuffle.")
S5=$(addsec "Selectable text & nav" "Canvas text islands opt into user-select:text so a left-drag selects and copies. Section rows are interactive: single-click highlights connected task edges + nodes (no camera move); double-click frames them. Rows + cards expose go-to-tasks, copy-id, open-tab, focus-stride affordances at W2/W3.")
S6=$(addsec "Handoff lane" "Orchestrator handoffs render as reset coins on a chronological timeline lane above the plan, linked [n-1]->[n]. W0 coin -> W1 goal+timestamp -> W2 next_actions + in-flight agents + session chips -> W3 expanded body + ack state. Latest/pending pulses; acked recedes spatially, never content-faded.")
S7=$(addsec "Camera discipline" "Live changes NEVER move the user's camera. A new task, stride, or handoff updates the node IN PLACE; the camera transform stays byte-identical. Off-screen changes surface a directional edge marker — clicking it is a user-initiated fly-there.")
S8=$(addsec "Uncovered work" "This section is intentionally left without tasks to verify the uncovered glyph + count surfaces in the compressed W0 view and the plan coverage bar.")

# task: name type model why body "sections" status priority(p0..p3) [parent]
mktask(){
  local name="$1" type="$2" model="$3" why="$4" body="$5" secs="$6" st="$7" prio="$8" parent="${9:-}"
  local payload tid
  payload=$(jq -n --arg n "$name" --arg ty "$type" --arg m "$model" --arg w "$why" --arg b "$body" \
    --arg s "$secs" --arg p "$prio" --arg par "$parent" --arg h "$name" \
    '{name:$n,type:$ty,body:$b,sections:$s,priority:$p,handle:($h|split(":")[0])}
     + (if $m=="" then {} else {model:$m} end)
     + (if $w=="" then {} else {why:$w} end)
     + (if $par=="" then {} else {parent:$par} end)')
  tid=$(jpost /api/plans/$PID/tasks "$payload" | jq -r '.task_id // .id')
  [ -n "$tid" ] && [ "$st" != pending ] && jpatch /api/plans/$PID/tasks/$tid "$(jq -n --arg s "$st" '{status:$s}')"
  echo "$tid"
}
stride(){ jpost /api/plans/$PID/tasks/$1/milestones "$(jq -n --arg k "$2" --arg t "$3" '{kind:$k,text:$t}')" >/dev/null; }
setstatus(){ jpatch /api/plans/$PID/tasks/$1 "$(jq -n --arg s "$2" '{status:$s}')"; }

T1=$(mktask "U1: auto-refresh on timer expiry" implement claude-opus-4-8 "deep change needs the strongest model" "Reset the usage tabl proactively when a reset timer expires — no manual click. Hook the poller to fire a refresh at the boundary and re-render the cycle." "$S1,$S2" in_progress p0)
stride "$T1" status_update "wired the poller boundary trigger"
stride "$T1" decision "reset at boundary, not on focus — avoids stale windows"
stride "$T1" testing "added a fake-clock test crossing the boundary"
stride "$T1" status_update "verifying the re-render path"

T2=$(mktask "Stride snake renderer" implement claude-opus-4-8 "" "Draw the boustrophedon snake of kind-colored beads with semicircular elbow turns; bloom to cards at W3 with the path as a spine." "$S3" in_progress p1)
stride "$T2" status_update "boustrophedon geometry done"
stride "$T2" review "looks like a thread, not square blocks"
stride "$T2" testing "beads render as circles across bands"
stride "$T2" decision "freshest bead pulses as the head"
stride "$T2" status_update "bloom layout reserves space"
stride "$T2" status_update "labels alternate above/below"

T3=$(mktask "Task cloud packing" implement claude-opus-4-8 "" "Pack tasks as a 2D cloud right of the plan, sorted by status then priority; bounded height, overflow rightward." "$S4" done p1)
stride "$T3" status_update "column-major fill bounded by cloudMaxH"
stride "$T3" testing "no overlap; done sinks below active"
setstatus "$T3" done

T4=$(mktask "Selectable canvas text" implement claude-sonnet-4-6 "" "Add .ct-canvas-text islands so a left-drag selects and copies; pan stays on middle/space." "$S5" done p2)
stride "$T4" status_update "css split shipped"
setstatus "$T4" done

T5=$(mktask "Section nav + affordances" implement claude-opus-4-8 "" "Single-click highlights connected tasks; double-click frames; copy-id and go-to buttons at W2/W3. No camera move on single-click." "$S5" pending p1)
stride "$T5" status_update "highlight state wired"

T6=$(mktask "Handoff coin renderer" implement claude-opus-4-8 "" "Render kind=handoff as a coin on the timeline lane with W0-W3 detail and ack state." "$S6" pending p2)

T7=$(mktask "Camera byte-identical guard" review claude-opus-4-8 "" "Assert the camera transform is byte-identical after any live data mutation." "$S7" blocked p1)
stride "$T7" decision "blocked on a flaky resize observer in CI"

T8=$(mktask "Coverage rollup" implement claude-haiku-4-5-20251001 "cheap mechanical rollup" "Two-hop coverage: spec section -> plan section -> tasks, rolled daemon-side." "$S2" done p3)
setstatus "$T8" done

T9=$(mktask "Footprint formula" implement claude-opus-4-8 "" "Two-axis bounded content-driven size with an aspect floor so cards are never hollow letterboxes." "$S2,$S4" done p2)
setstatus "$T9" done

# subtasks under T2 (snake) — exercises the volume cue + subtask cards
SUB1=$(mktask "bead geometry" implement "" "" "Compute boustrophedon bead positions per band." "$S3" done p2 "$T2")
setstatus "$SUB1" done
SUB2=$(mktask "elbow arcs" implement "" "" "Semicircular turns at row edges; coil for loops." "$S3" in_progress p2 "$T2")
SUB3=$(mktask "W3 bloom cards" implement "" "" "Beads bloom into full stride micro-cards." "$S3" pending p2 "$T2")

# standalone-ish + more done to fill the cloud bottom
T10=$(mktask "Workspace isolation" research claude-sonnet-4-6 "" "Scope the board to one project root." "$S7" done p3)
setstatus "$T10" done
T11=$(mktask "Edge routing polish" implement claude-sonnet-4-6 "" "Fan near-parallel row beziers so each connection traces." "$S5" cancelled p3)
setstatus "$T11" cancelled

# ── handoffs on the timeline lane ──
jpost /api/plans/$PID/handoffs "$(jq -n '{goal:"Ship §32 WORLDS v2: orthogonal channels, cloud, snake, handoff lane", next_actions:["finish section nav","render handoff coins","run the §32.9 matrix"], narrative:"Handoff #1 — first reset after the channel rework landed. Cloud + snake in place; nav + handoff render remain.", mode:"new", foundational_rules:["live changes never move the camera","status by position, not opacity"]}')" >/dev/null
jpost /api/plans/$PID/handoffs "$(jq -n '{goal:"Polish to Figma-grade and dogfood the ergonomics", next_actions:["verify done sinks","check selectable text","screenshot the board"], narrative:"Handoff #2 — second reset; verification phase.", mode:"new"}')" >/dev/null

# Claim the plan to the active workspace's tab so the per-tab Plan mode surfaces
# the board (PlanButton only shows for a tab that owns / works the plan).
TAB=$(curl -s "${A[@]}" "$B/api/workspaces" | jq -r 'if type=="array" then . else .workspaces end | map(select(.is_active==true))[0].tab_ids[0] // empty')
[ -n "$TAB" ] && jpost /api/plans/$PID/claim "$(jq -n --arg p "$TAB" '{owner:{tab_id:$p,session_id:null,title:"worlds2"}}')" >/dev/null && echo "claimed plan to tab $TAB"

echo "done:"
curl -s "${A[@]}" "$B/api/plans?project=$PROJ&full=1" | jq -r --arg p "$PID" '.plans[]|select(.id==$p)|"sections=\(.sections|length) tasks=\(.tasks|length) strides=\([.tasks[].milestones[]]|length) handoffs=\([.elements[]|select(.kind=="handoff")]|length)"'
