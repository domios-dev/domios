#!/usr/bin/env bash
# Comprehensive Canvas v2 dogfood — the whole journey as a plan with full section bodies.
set -uo pipefail
PROJ="/Users/rollacode/Developer/codeterm"
export CODETERM_API_PORT=7686
TOKEN=$(grep -E '^token = ' ~/.codeterm-dev/config.toml | head -1 | sed 's/.*"\(.*\)"/\1/')
B="http://127.0.0.1:7686"; A=(-H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json")
jpost(){ curl -s "${A[@]}" -X POST "$B$1" -d "$2"; }
sec(){ jq -n --arg t "$1" --arg b "$2" '{title:$t,body:$b}' ; }
addsec(){ jpost /api/plans/$PID/sections "$(sec "$1" "$2")" | jq -r '.section_id // .id'; }

PID=$(jpost /api/plans "$(jq -n --arg p "$PROJ" '{title:"Canvas v2 — the full build (dogfood)",project_path:$p,kinds:"status_update,decision,testing,review"}')" | jq -r '.plan_id // .id')

S1=$(addsec "Concept & vision" "Canvas is a Figma-like 2D board, scoped per workspace, that visualizes live multi-agent orchestration: the orchestrators in a workspace, the **plans** they own, and for each plan the cascade Spec → Tasks → chain of report-milestones, looping into self-improving processes. It is the spatial, persistent counterpart to orchestration that today lives only as ephemeral chat + the report ledger. Two entry points: click a workspace for the overview of all plans, or the per-tab **Plan** button to focus on what that agent works on.")
S2=$(addsec "Data model" "A Plan nests Sections (a tree, each with markdown body), Tasks (name, type, model, model_rationale, status, status_label, section_ids many-to-many, body, milestones), and Milestones/Steps (kind, text, links, loop_id, iteration). Identity is stable across restart (session_id over tab_id). Every entity carries its own ElementCanvas position. Source of truth = per-project JSON at <project>/.codeterm/plans/<id>.json; an in-memory index enables fast cross-project listing; the daemon is the single writer (atomic temp-write + rename).")
S3=$(addsec "Layout model (the big v1 fix)" "v1's spec said 'a plan is a frame whose contents are section-cascade-left → task-rows → milestone-columns' — rendered, that produced a giant box with three disconnected bands: scattered sections, floating tasks, milestones flying off with no edges. v2 replaces it: one COMPACT sectioned plan node (the hero), a separate SPEC node, satellite TASK cards joined by curved edges to their exact section rows, and STEP bubble CLUSTERS hung off each task. Nothing floats. Coverage shows as a fill tint on covered rows (amber pops on uncovered). Deterministic auto-pack — no manual dragging — so realtime additions slot in cleanly.")
S4=$(addsec "Navigation (Figma-grade)" "Trackpad two-finger scroll pans both axes; Cmd/Ctrl+scroll zooms around the pointer; pinch zoom works via the synthesized ctrlKey wheel; middle-button and space+drag pan as fallback. The wheel listener is non-passive so the page never rubber-bands. fit-to-view frames the real content bbox on mount and fills the viewport. All smooth via requestAnimationFrame batching.")
S5=$(addsec "Workspace isolation" "One workspace = one project root (git toplevel of the cwd) = one .codeterm/plans directory. The canvas in a workspace shows ONLY plans whose project_path canonicalizes to that root. This fixes a real leak where plans from one project bled into another workspace's board. Verified: the dogfood appears in the codeterm project and is absent from a sibling project's canvas.")
S6=$(addsec "Standalone tasks (Inbox)" "Tasks may exist without a plan. A plan-less task lands in a reserved per-project Inbox plan (id 'inbox'), reusing ALL plan machinery — no new storage path. A task can be taken by a worker OR an orchestrator (assigned_tab_id + derived taken_by_role); a link chip on the card opens that tab. This generalizes Task to be a first-class, addressable unit.")
S7=$(addsec "Graph / Task CLI + canvas focus" "New surface over the plan store (NOT mem-graph; plans stay JSON): codeterm task list/show/take/drop/add for 'what is in flight / what can I pick up / take this', plan gaps for 'what is uncovered', and codeterm canvas focus --task|--tab|--plan which emits a canvas-focus event over the existing emit_everywhere fan-out so the open board pans+zooms+blinks the target. The per-tab Plan button uses this to focus the camera on the area that agent works on.")
S8=$(addsec "Plan-as-mode + composer + activity bubble" "Plan is a THIRD tab mode peer to Chat and Terminal — selecting it highlights the button and renders the canvas in the tab body; Chat/Terminal returns. Label is role-driven (Plan for orchestrators, Tasks for workers). A floating talk-to-area composer is context-addressed to the orchestrator owning the focused region and clears on send; a small non-modal activity bubble shows that orchestrator's live status so you can see something is happening.")
S9=$(addsec "Persistence (rescan on restart)" "A critical bug: plans on disk were not re-indexed after a daemon restart — the list only filtered the in-memory map and never scanned the directory, and watchers were not retained for pre-existing project dirs. Fix: list-by-project lazily scans <project>/.codeterm/plans, startup scans open-workspace CWDs, watchers cover scanned dirs. Plans now survive restart; verified by restarting dev and reloading the board with no re-seed.")
S10=$(addsec "Coverage model" "Coverage is derived, never stored: per section = count of tasks referencing it + a status rollup (none/pending/in_progress/done); plan-level = fraction of sections with >=1 task. Rendered as a fill tint on each covered row, a strong amber+dashed treatment on uncovered rows (so gaps pop), and a labeled footer '2/2 tasks · 86% covered'. Surfaced on the CLI via plan coverage / plan gaps.")
S11=$(addsec "The overnight iteration loop" "The board was polished from 'structurally correct' to 'Figma-grade' over 10 build iterations, each design-reviewed by a fresh independent critic using the frontend-design skill, scored against a 10-dimension rubric. Roles: Mira (lead designer-engineer, opus), Dax (adversarial critic, opus), Rex (mechanical fixer, codex). Cycle: build → integrate → per-zone Playwright screenshot → independent score → fold critique → repeat. The loop caught real over-corrections (edges loud↔invisible, spec demoted↔chromeless) a single builder could not self-see.")
S12=$(addsec "Infra lessons (learned the hard way)" "Two recurring traps cost real time. (1) A worker that finishes but never commits, then has its worktree force-removed, loses everything — preserve before cleanup. (2) Symlinking node_modules into a worktree where an agent runs pnpm install corrupts the main node_modules into a self-referential symlink, breaking tsx repo-wide with exit 194 — give each worktree its own real node_modules and never commit node_modules. Both are now guarded.")

task(){ # name type model why body sections... (space-sep var refs) status
  local n=$1 ty=$2 mo=$3 wy=$4 bo=$5 sids=$6 st=$7; shift 7
  local tid; tid=$(jpost /api/plans/$PID/tasks "$(jq -n --arg n "$n" --arg ty "$ty" --arg mo "$mo" --arg wy "$wy" --arg bo "$bo" --arg s "$sids" '{name:$n,type:$ty,model:$mo,why:$wy,body:$bo,sections:$s}')" | jq -r '.task_id // .id')
  for m in "$@"; do jpost /api/plans/$PID/tasks/$tid/milestones "$(jq -n --arg k "${m%%:*}" --arg t "${m#*:}" '{kind:$k,text:$t}')" >/dev/null; done
  [ "$st" = done ] && curl -s "${A[@]}" -X PATCH "$B/api/plans/$PID/tasks/$tid" -d '{"status":"done"}' >/dev/null
  [ "$st" = in_progress ] && curl -s "${A[@]}" -X PATCH "$B/api/plans/$PID/tasks/$tid" -d '{"status":"in_progress"}' >/dev/null
}

task "Mira — frontend rebuild" implement "claude:opus" "design-critical: frontend-design skill + layout/nav reasoning" \
  "Rebuilt the whole Canvas frontend: compact sectioned plan node, spec node, curved section-row→task edges, step bubble clusters, orchestrator groups, trackpad navigation, Plan-as-mode, composer + activity bubble, no-drag deterministic auto-pack, coverage tint, markdown, bounds discipline. Drove 10 polish iterations." \
  "$S3,$S4,$S8,$S10" done \
  "decision:Root cause of the ugly board = v1's frame-with-3-bands layout; replaced with compact sectioned node + connected satellites" \
  "review:Killed the floating clusters (root-cause cluster placement), legible edges, captioned bubbles" \
  "testing:tsc clean, 13 canvas test files green across all iterations"
task "BE — tasks/CLI/isolation" implement "codex:gpt-5.5" "mechanical backend wiring over the JSON store; faster" \
  "Standalone tasks via the reserved Inbox plan, codeterm task list/show/take/drop/add, plan gaps, codeterm canvas focus emission, strict workspace project-root scoping, archive cascade. New REST + socket + CLI surface, manifest regenerated." \
  "$S5,$S6,$S7" done \
  "decision:Reuse plan machinery (Inbox plan) for standalone tasks rather than a new store" \
  "testing:cargo plan 43 + task 35 green; manifest fresh"
task "Rex — persistence fix" fix "codex:gpt-5.5" "focused fast fix on a real regression" \
  "Plans on disk were not re-indexed after restart. Added lazy project-dir scan on list, startup scan of open-workspace CWDs, retained watchers for pre-existing dirs, plus a regression test for a directly-dropped plan JSON." \
  "$S9" done \
  "decision:List-by-project never scanned the dir; watchers not retained after restart — both fixed" \
  "testing:cargo test plan 45 green; verified plans survive restart with no re-seed"
task "Dax — independent design reviews" review "claude:opus" "adversarial fresh eyes per iteration via frontend-design skill" \
  "Six independent reviews scoring the live board screenshots against the 10-dim rubric, each producing a ranked concrete defect list. Caught over-corrections the builder could not self-see (edges loud↔invisible, spec demoted↔chromeless, coverage tint wall-of-green↔invisible)." \
  "$S11" done \
  "review:iter4=80 iter5=76 iter6=80 iter7=74 iter8=79 iter9=72 — score was noise; defect-list convergence was the real signal" \
  "decision:Stop the fresh-critic treadmill at iter10 once guidance turned contradictory — ship"
task "Orchestrator — drive the loop" implement "claude:opus" "EM/PM coordination, integration, screenshot+score, dogfood" \
  "Designed v2, decomposed into lanes, spawned + coordinated the workers, ran the integrate→screenshot→score→critique loop, preserved lost work, neutralized infra traps, dogfooded the plan onto its own canvas." \
  "$S1,$S2,$S12" done \
  "status_update:10 iterations, 6 independent reviews, board shipped Figma-grade" \
  "decision:Identified the recurring Connection/Packing gap as a layout-algorithm root cause, not symptoms to keep patching"

echo "PLAN_ID=$PID"
curl -s "${A[@]}" "$B/api/plans?project=$PROJ&full=1" | jq -r --arg p "$PID" '.plans[]|select(.id==$p)|"sections=\(.sections|length) tasks=\(.tasks|length) milestones=\([.tasks[].milestones[]]|length)"'
