---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_9e88d51dfd0d40aab43dca58d1ed141e
handle: t6-e2e-gate
title: Playwright visual/e2e gate for plan views
status: pending
---

Playwright visual/e2e gate for the redesigned plan views (macOS web-mode against dev 7686). Runs after t1,t2,t2b,t3,t4,t5 are integrated in the integration checkout.

Steps:
1. Update suites/13-canvas/plan-view-mode.spec.ts: Board default, order Board/List/Canvas, persistence across close/reopen.
2. New specs (same suite dir): detail overlay opens from Board card, List row, Canvas card; Esc restores prior state (board scroll, list scroll, canvas camera).
3. Done-heavy assertions using the 200-done fixture: bounded board page height, Done column collapsed w/ working "Show all", list done-only sections collapsed to rollups.
4. Screenshot artifacts: Board, List, Canvas, spec card + spec detail, on normal AND done-heavy fixtures -> tests/playwright/artifacts/; list exact paths in the report (these go to the user for visual acceptance).
5. Tag hygiene: npm run test:pw:lint.

Commands: npx playwright test -c tests/playwright --project=golden; targeted runs per spec while authoring; npm run test:pw:lint.
Report: pass/fail per spec, artifact paths, any flakiness quarantined per tests/playwright/QUARANTINE.md conventions.
