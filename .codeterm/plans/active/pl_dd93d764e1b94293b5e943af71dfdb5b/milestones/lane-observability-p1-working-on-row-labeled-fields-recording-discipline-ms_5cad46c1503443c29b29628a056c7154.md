---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_5cad46c1503443c29b29628a056c7154
handle: lane-observability-p1-working-on-row-labeled-fields-recording-discipline-ms_5cad46c1503443c29b29628a056c7154
title: status_update
---

PARTIAL: STATUS: partial (my work COMMITTED clean at 7f651657; dirty files listed by the guard belong to a CO-TENANT lane sharing this worktree — readback.rs/plan_ops.rs/search_index*/readback_tests.rs — I did NOT touch or stage them).

DONE (task_03bf8da6 — lane observability P1):
- Working-on row on Board cards + List rows for in_progress/in_review from EXISTING lane fields only: model glyph, pane (title/short-id), mesh peer chip, branch, activity age. Shared helper deriveWorkingOnRow + WorkingOnRowView.
- Labeled fields replace raw #handle noise on working cards.
- Honest fallbacks: 'no pane recorded' (amber), 'branch missing' (amber), 'pane gone' (red, absent locally + no peer); remote pane keeps grey short-id + peer chip, never 'gone'.
- Recording discipline (the actual fix): PlanStore::fill_task_lane_if_empty — agent report fills EMPTY lane pane/branch from caller pane + report BRANCH, never overwrites. Wired into hierarchy submit_report.

DONE (task_1d05 — watchdog core, minimal):
- Shared plan::lane_liveness: derive_lane_liveness -> Working/IdleStalled/ReportOverdue/PaneGone/PeerOffline/NoPane (peer-offline parks; pane-gone only w/o live peer; overdue amber). verdict_for -> flag/stride/notify, NEVER a status transition. Single helper for reconcile+watchdog+plan-lanes.

DONE (Andrey add-on — sidebar reconciliation):
- Regression test hierarchyPaneGone.test.ts: closed worker absent from pane list is not rendered as a live child even with a lingering task.assigned_pane_id. Reducer SET_STATE (L89) rebuilds panes wholesale -> stale pruned. ROOT-CAUSE FLAG: residual leak is DAEMON-side (reaped pane still in mux-state snapshot while pane API says not-found) -> needs a separate lane.

GATES:
- Frontend VERIFIED green (tsx): workingOnRow 7/7, BoardTaskCard 2/2, PlanBoardView 17/17, PlanListView 19/19, hierarchyPaneGone 1/1.
- Rust NOT compiled: cargo test blocked on shared .cargo-lock, then stopped per your STOP order. Intended focused QA gates: 'cargo test plan::lane_liveness', 'cargo test fill_lane_tests', compile check over hierarchy.rs + mutations/task_ops.rs.

FOLLOWUP: watchdog LIVE loop (60s sweep + agent_tracker subscription + Messenger ping/notify + config) NOT wired — only pure core. Report-refusal branch+pane enforcement rides prompt lane task_dc526c35. Watcher self-close/'watch:' convention is planner/skill scope. Mesh-origin peer capture in fill_task_lane_if_empty is a follow-up (passes None on local reports).
