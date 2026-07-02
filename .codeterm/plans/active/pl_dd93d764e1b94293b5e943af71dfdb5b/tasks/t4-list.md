---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_75995d413fab46a69dc1b3fd1df93cfc
handle: t4-list
title: List redesign (secondary view)
status: pending
---

Redesign List into the secondary orientation/audit view. Depends on t2-detail-overlay and t3-board (PlanContextHeader reuse). Read spec sections list-req, done-req, context-req first.

Steps:
1. Interactivity: task rows click -> PlanDetailOverlay(task); section rows toggle collapse; spec-root/plan-root rows -> overlay (spec/plan). Keyboard: Enter on focused row.
2. Collapse model: extend buildPlanListTree.ts or add planView/listViewState.ts — expand-state map, done-only sections default collapsed, "n done" rollup rows for collapsed done groups. Unit tests in buildPlanListTree.test.ts.
3. Quiet done styling (dimmed, compact); thin toolbar with status filter chips (all/active/done/blocked) + text filter; sticky plan header via PlanContextHeader.
4. Keep row density (no cards). Preserve existing plan-list-* testids; add plan-list-collapse-toggle, plan-list-filter, plan-list-done-rollup.
5. Update PlanListView.test.tsx.

Commands: npm run test:unit; dev visual pass on normal + done-heavy fixture; 2 screenshots for the report.
Report: screenshots, collapse/filter model description, files changed, test output.
