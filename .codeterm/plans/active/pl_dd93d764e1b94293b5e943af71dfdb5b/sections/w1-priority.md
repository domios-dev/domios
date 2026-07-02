---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: section
id: sec_41bc141f58524a3086504f52a429215c
handle: w1-priority
title: Wave 1 — View priority and mode persistence
---

Wave 1 (unblocks everything visually cheap): reorder plan view modes to Board, List, Canvas; make Board the default; persist the chosen mode.

File ownership:
- src/components/Canvas/planViewMode.ts — PLAN_VIEW_MODES order, DEFAULT_PLAN_VIEW_MODE = "board".
- src/components/Canvas/PlanViewShell.tsx — persist mode (localStorage key e.g. codeterm.planViewMode via a small usePersistedPlanViewMode hook; per-user, no backend).
- src/components/Canvas/PlanViewModeSwitch.tsx — order follows PLAN_VIEW_MODES.
- Tests: planViewMode.test.ts, PlanViewShell.test.tsx, tests/playwright/suites/13-canvas/plan-view-mode.spec.ts.

Verification: npm run test:unit; npx playwright test -c tests/playwright --project=golden (plan-view-mode spec updated for new default/order).
Risk: existing e2e assumes canvas default — the spec must be updated in the same change, not skipped.
