---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_e7ef4c2763fd4fd699bbf3a6f525831b
handle: t3-board
title: Board redesign (primary view)
status: pending
---

Redesign Board into the primary work surface. Depends on t1-view-order (board default) and t2-detail-overlay (click-through target). Read the spec sections board-req, done-req, context-req first (codeterm spec show sp_d2c70550432f4a829c89cf4a4cb7c3c5).

Steps:
1. New planView/PlanContextHeader.tsx replacing BoardEntitySummary cards: plan title, goal vector (truncated + expand), heartbeat focus, owner, done/total progress, spec title + section count; click -> PlanDetailOverlay (plan or spec kind). Design for reuse by List (t4-list).
2. PlanBoardView.tsx + globals.css .plan-board-*: columns fill width, independent column-body scroll with sticky column headers, subtle empty-column placeholder, calm visual hierarchy using var(--ct-*) tokens only.
3. boardModel.ts: done-column bounding — collapsed default (5 most recent + total count), explicit "Show all N" toggle, expansion scrolls in-column; unit-test the model (boardModel.test.ts).
4. BoardTaskCard.tsx: whole card clickable -> PlanDetailOverlay(task); hover/focus affordance; keep handle/status/presence/lane/milestone facts with title-first hierarchy. Preserve existing testids; add plan-board-done-toggle, plan-context-header.
5. Update PlanBoardView.test.tsx (click opens overlay, done collapse, header present).

Commands: npm run test:unit; visual self-check in dev (7686) on a real plan + perfFixture done-heavy plan; capture 2 screenshots for the report.
Report: screenshots, files changed, model changes, testids added, any UX decision you made beyond spec (flag it for review).
