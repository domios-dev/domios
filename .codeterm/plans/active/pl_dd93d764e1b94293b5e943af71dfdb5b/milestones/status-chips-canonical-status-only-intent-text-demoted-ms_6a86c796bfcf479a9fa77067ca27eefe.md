---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_6a86c796bfcf479a9fa77067ca27eefe
handle: status-chips-canonical-status-only-intent-text-demoted-ms_6a86c796bfcf479a9fa77067ca27eefe
title: status_update
---

STATUS: in_review
CHANGES: lane/status-chip-canonical @ c89910e8 — taskStatusLabel.ts, BoardTaskCard.tsx, boardModel.ts, DetailContent.tsx, buildPlanListTree.ts, taskQa.ts, globals.css + BoardTaskCard.test.tsx, taskStatusLabel.test.ts + updated board/list/detail tests
BUILD/TYPECHECK/TESTS: not run (worker charter). Verified: npx tsx taskStatusLabel.test.ts (2/2), boardModel.test.ts (14/14), buildPlanListTree.test.ts (15/15), BoardTaskCard.test.tsx (2/2), PlanDetailOverlay.test.tsx (15/15), PlanBoardView.test.tsx (16/16)
FOLLOWUP: Orchestrator build/UI gate. Playwright golden for status-chip text still pending per task test_plan. Replaced plan-board-task-status testid with status-chip on cards; detail header Chip also uses status-chip. plan record OK; agent report retried after plan_index_mirror lock.
