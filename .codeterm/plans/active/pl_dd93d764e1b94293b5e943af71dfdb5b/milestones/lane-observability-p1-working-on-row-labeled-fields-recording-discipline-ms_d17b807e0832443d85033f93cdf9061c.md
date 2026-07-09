---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_d17b807e0832443d85033f93cdf9061c
handle: lane-observability-p1-working-on-row-labeled-fields-recording-discipline-ms_d17b807e0832443d85033f93cdf9061c
title: report_refused
---

PARTIAL: REPORT NOT SENT
readiness report refused: this git worktree has uncommitted lane changes.
Next commands:
  git status --short
  git add <files>
  git commit -m "<message>"
  codeterm agent report --status in_review --sha $(git rev-parse HEAD) "..."
Or report WIP with:
  codeterm agent report --status partial "..."
Dirty lane files:
  src-tauri/src/service/plan/mod.rs
  src-tauri/src/service/plan/mutations/mod.rs
  src-tauri/src/service/plan/mutations/plan_ops.rs
  src-tauri/src/service/plan/readback.rs
  src-tauri/src/service/plan/search_index.rs
  src-tauri/src/service/plan/search_index/tests.rs
  src-tauri/src/service/plan/readback_tests.rs

Original readiness report that was refused locally:
STATUS: in_review (partial across two tasks; core landed)
BRANCH: lane/observability-p1-working-on  SHA: 7f651657

DONE (task_03bf8da6 — lane observability P1):
- Working-on row on Board cards + List rows for in_progress/in_review, built from EXISTING lane fields only: model glyph, pane (title/short-id), mesh peer chip, branch, activity age. New shared helper deriveWorkingOnRow + WorkingOnRowView component.
- Labeled fields replace the raw #handle noise on working cards (branch icon+name; status stays its own chip).
- Honest fallback chips: 'no pane recorded' (amber, no assigned pane), 'branch missing' (amber), 'pane gone' (red, pane recorded but absent locally with no peer), remote pane keeps grey short-id + peer chip (never 'gone').
- Recording discipline (the actual fix): PlanStore::fill_task_lane_if_empty — agent report captures pane id (caller CODETERM_PANE_ID) + branch (report BRANCH line) into EMPTY lane fields only, never overwrites. Wired into hierarchy submit_report local-plan path.

DONE (task_1d05 — watchdog core, minimal):
- Shared plan::lane_liveness helper: derive_lane_liveness -> Working/IdleStalled/ReportOverdue/PaneGone/PeerOffline/NoPane (peer-offline parks verdict; pane-gone only without live peer; report-overdue amber not gone). verdict_for -> flag/stride/notify. Verdict NEVER carries a task-status transition (watchdog flags+records+notifies only). Designed as the single helper reconcile + watchdog + plan-lanes share.

DONE (Andrey add-on — sidebar reconciliation):
- Regression test hierarchyPaneGone.test.ts: a closed worker pane absent from the pane list is NOT rendered as a live child under the orchestrator, even when a plan task still carries its assigned_pane_id. Confirmed buildHierarchy* only render live panes; SET_STATE (reducer L89) rebuilds panes wholesale so stale panes ARE pruned once the daemon snapshot drops them. ROOT-CAUSE FLAG: if a reaped pane still shows, it is because the daemon MUX SNAPSHOT still lists it (pane API returns not-found but mux-state includes it) — that is a DAEMON-side leak, not frontend; needs a separate lane to prune reaped panes from the mux snapshot. Plan-surface pane-gone chip is delivered here.

GATES:
- Frontend (VERIFIED green locally, tsx): workingOnRow 7/7, BoardTaskCard 2/2, PlanBoardView 17/17, PlanListView 19/19, hierarchyPaneGone 1/1.
- Rust (NOT run to completion): cargo test blocked on the shared .cargo-lock, then stopped per your 'STOP heavy local gates' order. UNVERIFIED by compile. Intended focused gates for QA: 'cargo test plan::lane_liveness' , 'cargo test fill_lane_tests', and a compile check covering hierarchy.rs + mutations/task_ops.rs.

NOT DONE / FOLLOWUP:
- Watchdog LIVE wiring (60s sweep over active lanes, agent_tracker status subscription, PTY-activity events, Messenger ping-once + orchestrator notify via plan heartbeat, config [orchestration.lane_watchdog]) — only the pure derivation+verdict core is landed; the loop is the remaining integration.
- Recording discipline 3b (worker prompt report-refusal enforcing branch+pane) rides the prompt-surgery lane task_dc526c35 — coordinate there, not duplicated here.
- Watcher hygiene self-close tail + 'watch:' title convention is planner-TEXT-D / orchestrator-skill scope (no repo file); sidebar dim of watch: panes not implemented.
- Peer detection for fill_task_lane_if_empty passes None on local reports (peer stays as-is); mesh-origin peer capture is a follow-up.

CO-EXISTING: this worktree is shared — another lane's uncommitted plan-update readback changes (service/plan/readback.rs, mutations/plan_ops.rs, search_index.rs, readback_tests.rs) are present but I did NOT stage or touch them.
