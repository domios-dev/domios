---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_3844057c4ab2427eac0d034ffc07b47e
handle: fix-r4-bound-planlistview-done-history-dom-under-filters-ms_3844057c4ab2427eac0d034ffc07b47e
title: status_update
---

STATUS: in_review
CHANGES: planListModel.ts (boundDoneFilterVisibleRows + DONE_LIST_FILTER_VISIBLE_LIMIT alias), buildPlanListTree.ts (done-filter-toggle row type), PlanListView.tsx (doneFilterExpanded state + cap wiring), PlanListRowView.tsx (Show all/recent toggle), planListModel.test.ts + PlanListView.test.tsx (250 done task coverage)
BUILD/TYPECHECK/TESTS: npx tsx planListModel.test.ts (3/3), PlanListView.test.tsx (19/19), buildPlanListTree.test.ts (15/15)
FOLLOWUP: Branch lane/fix-plan-list-done-cap pushed at 1c5a8c48 from worktree ../codeterm-fix-plan-list-done-cap. Done filter now caps to DONE_COLUMN_COLLAPSED_LIMIT (5) with Show all N / Show recent 5 toggle; small done sets unchanged.
