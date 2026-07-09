---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: section
id: sec_649d5f05b44a40d294c18e03c68dc7ab
handle: w3-board
title: Wave 3 — Board redesign
---

Wave 3: Board redesign — the primary view. Depends on Wave 2 (shared overlay) for card click-through; can start layout/CSS work in parallel after Wave 1.

Scope:
- Plan-context header band (new shared component, e.g. planView/PlanContextHeader.tsx) replacing the two floating summary cards: title, goal vector (truncate + expand), focus, owner, done/total progress; click -> plan/spec detail via shared overlay. Also consumed by List (Wave 4).
- Column redesign in globals.css .plan-board-* + PlanBoardView.tsx: independent column scroll, sticky column headers, subtle empty placeholders, full-width layout.
- Done column bounded: default recent 5 + count badge + "Show all N" toggle scrolling in-column (boardModel.ts gains doneVisibleCount/collapse model with unit tests).
- BoardTaskCard: clickable (opens PlanDetailOverlay), hover/focus affordance, typographic hierarchy pass.

File ownership: planView/PlanBoardView.tsx, planView/boardModel.ts, planView/BoardTaskCard.tsx, planView/BoardEntitySummary.tsx (likely deleted/absorbed into PlanContextHeader), planView/PlanContextHeader.tsx (new), src/styles/globals.css (plan-board-* rules), tests PlanBoardView.test.tsx + boardModel.test.ts.
Verification: npm run test:unit; Playwright board specs (Wave 6 owns e2e, but the implementer must keep testids stable and add new ones: plan-board-done-toggle, plan-context-header, plan-board-task-card clickable).
