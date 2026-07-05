---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_7ea63fc8bc5147deb8ca8bf36103d37a
handle: show-historical-executors-separately-from-active-presence-ms_7ea63fc8bc5147deb8ca8bf36103d37a
title: status_update
---

PARTIAL: STATUS: partial
CHANGES: Added taskExecutorHistory module (resolve attempts from execution_attempts wire field or legacy lane/assignment), separated active presence vs historical executor rendering in Board/List/Detail/card surfaces, stable data-testid hooks (plan-board-task-active-executor, plan-board-task-historical-executor, plan-list-active-executor, plan-list-historical-executor, canvas-detail-executor-history). No resume affordance (respawn API not landed yet). Lane: lane/task-history-ui worktree.
BUILD/TYPECHECK/TESTS: npx tsx taskExecutorHistory.test.ts, taskPresence.test.ts, boardModel.test.ts, buildPlanListTree.test.ts, cardContent.test.ts, PlanBoardView.test.tsx — all passed. tsc has pre-existing plugin-sdk errors unrelated to this diff.
FOLLOWUP: Commit lane/task-history-ui before in_review gate. Backend tasks (execution_attempt schema + respawn CLI/API) still pending; wire resume button when API exists. Index assigned_pane_ids for terminal tasks may still drive tab bubbles until backend clears live assignment on close.
