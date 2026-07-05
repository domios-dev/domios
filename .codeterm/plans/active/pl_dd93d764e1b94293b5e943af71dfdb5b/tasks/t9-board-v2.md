---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_2840fbfab62e4e20bf77682909294af6
handle: t9-board-v2
title: 'Board v2: single-plan switcher + card cleanup'
status: done
---

Board v2 (round-2 feedback, iteration 1/3 of the replanning loop). Read spec section "Board — primary view requirements" ROUND 2 block + wave w9-board-v2 first.

Steps:
1. New planView/PlanSwitcher.tsx (design for reuse by List): segmented control <=3 plans, dropdown beyond; order plans by live activity freshness (live orchestrator/worker panes, latest heartbeat/stride/report ts — extend boardModel with an activityScore); DEFAULT = current orchestrator's active plan; manual selection persists for the session (in-memory/store, not localStorage).
2. PlanBoardView: render ONE selected plan; delete the multi-plan vertical stack path (workspace mode uses the switcher too).
3. Layout: board fills viewport height; page never scrolls; column bodies scroll independently (fix the current clipped-mid-card overflow bug — inspect .plan-board-column-body overflow/height chain), visible scrollbar affordance, sticky column headers.
4. BoardTaskCard v2: 2-line title clamp; status pill; one executor line; branch-only lane chip; REMOVE worktree paths AND raw milestone/status-update text — at most a tiny activity hint (kind + relative age, e.g. "status update · 2m"); full timeline in detail overlay only. Card stays fully clickable.
5. Tests: boardModel (activityScore, default selection, single-plan), PlanBoardView (switcher render/select, no stacking), BoardTaskCard (no path/milestone leakage). Update Playwright board specs (switcher testid plan-switcher, column scroll, card click) — coordinate with the live e2e lane if still open.

Commands: npm run test:unit; npx playwright test -c tests/playwright --project=golden; dev screenshots (normal + done-heavy + multi-plan workspace) attached to report.
Report: screenshots, activity-ordering semantics, files changed, testids added/changed.
