---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_11f78f182fff4d81aad8b7725af979ac
handle: quiet-tab-activity-indicator-and-remove-task-bubbles-from-tabs-ms_11f78f182fff4d81aad8b7725af979ac
title: status_update
---

IN_REVIEW: STATUS: in_review
CHANGES: TabActivityIndicator.tsx (unified 5px quiet dot for orch/worker, no glyph/pulse), TabItem.tsx (no provider working spin, hide description when plan activity present, data-tab-no-task-bubble), globals.css (.ct-tab-activity-quiet + sidebar tab icon animation suppress), TabActivityIndicator.test.tsx, TabItem.test.ts, tests/playwright/suites/13-canvas/quiet-tab-chrome.spec.ts (stable data-attr hooks)
BUILD/TYPECHECK/TESTS: pnpm exec tsc --noEmit -p tsconfig.json clean; TabActivityIndicator 9/9; TabItem source test pass; Playwright quiet-tab-chrome spec added but not run (orchestrator gate)
FOLLOWUP: orchestrator should merge/gate visual check — confirm tabs feel calm with real plan-assigned panes; run npx playwright test tests/playwright/suites/13-canvas/quiet-tab-chrome.spec.ts --project=golden when dev is up
