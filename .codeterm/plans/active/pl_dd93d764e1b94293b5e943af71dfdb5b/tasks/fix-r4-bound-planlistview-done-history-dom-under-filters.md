---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_cb72aea9597648af8c673e0c91637aa7
handle: fix-r4-bound-planlistview-done-history-dom-under-filters
title: 'Fix R4: bound PlanListView done-history DOM under filters'
status: done
---

HIGH review finding from R4 Canvas frontend surfaces. PlanListView renders visibleRows directly at src/components/Canvas/planView/PlanListView.tsx:345-366. The default done section is collapsed via defaultSectionExpanded in src/components/Canvas/planView/planListModel.ts:58-60, but buildVisiblePlanListRows forces every section open whenever statusFilter !== all or textQuery is set at lines 126-128. Selecting the Done filter therefore renders every done task row, so a plan with 200+ done tasks creates unbounded DOM despite the collapsed done rollup. Fix by preserving a bounded done-history policy under filters: keep a rollup plus recent N rows, add explicit Show all inside the scroll region, or virtualize the row list. The cheap fix is a cap/rollup in buildVisiblePlanListRows with tests for 200 done tasks.
