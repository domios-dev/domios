---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: section
id: sec_0aa19e12dc4e42388e9ee0f4abec866e
handle: w9-board-v2
title: 'Wave 9 — Board v2: single-plan + switcher (round 2)'
---

Wave 9 (round-2 replanning iteration 1/3 of loop_d8600689): Board v2 — single-plan board with switcher.

Scope (spec: Board round-2 block):
- Plan switcher in the board header (new planView/PlanSwitcher.tsx, shared with List): segmented <=3 plans, dropdown beyond; ordering by live orchestrator/worker activity freshness; default = current orchestrator's active plan; manual choice persists for the session. Kill vertical multi-plan stacking in PlanBoardView.
- Viewport-height board: page never scrolls; each column body scrolls independently with visible scrollbar affordance; sticky column headers. Fix the current clipped-mid-card rendering.
- BoardTaskCard v2: 2-line title clamp, status pill, one executor line, branch-only lane chip; REMOVE worktree paths and raw milestone/status-update text from cards — at most a tiny activity hint (kind + relative age) or nothing; full timeline in the detail overlay only.
File ownership: planView/PlanBoardView.tsx, PlanSwitcher.tsx (new), BoardTaskCard.tsx, boardModel.ts (activity ordering + default-plan selection), PlanContextHeader.tsx, globals.css plan-board-*.
Verification: npm run test:unit; updated Playwright board specs (switcher, single-plan render, column scroll, card click); screenshots for round-2 visual acceptance.
