---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_973127130e8c489586368c955e817fde
handle: t1-view-order
title: Board-first view order + persisted mode
status: done
---

Reorder plan view modes to Board (default), List, Canvas, and persist the selected mode.

Steps:
1. src/components/Canvas/planViewMode.ts: PLAN_VIEW_MODES = ["board","list","canvas"], DEFAULT_PLAN_VIEW_MODE = "board". Keep resolvePlanViewMode fallback semantics.
2. src/components/Canvas/PlanViewShell.tsx: replace bare useState with a usePersistedPlanViewMode hook (localStorage key "codeterm.planViewMode", validate via resolvePlanViewMode on read).
3. Verify PlanViewModeSwitch renders in the new order automatically (it maps PLAN_VIEW_MODES).
4. Update unit tests: planViewMode.test.ts, PlanViewShell.test.tsx (default board, persistence mock), PlanBoardView/PlanListView tests if they assume default mode.
5. Update tests/playwright/suites/13-canvas/plan-view-mode.spec.ts: default board, order Board/List/Canvas, persistence across plan close/reopen.

Commands: npm run test:unit; npx playwright test -c tests/playwright --project=golden (needs dev on 7686 + chromium installed).
Report: files changed, test output snippets, any e2e spec that had canvas-default baked in.
