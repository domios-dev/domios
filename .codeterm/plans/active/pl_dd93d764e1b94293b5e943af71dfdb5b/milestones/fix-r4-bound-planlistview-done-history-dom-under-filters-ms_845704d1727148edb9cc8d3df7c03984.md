---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_845704d1727148edb9cc8d3df7c03984
handle: fix-r4-bound-planlistview-done-history-dom-under-filters-ms_845704d1727148edb9cc8d3df7c03984
title: status_update
---

Accepted and merged at 1fdc38f9. Done-filter rendering is capped to recent 5 with Show all/recent toggle; small done sets unchanged. Gates: planListModel 3/3 PASS; PlanListView 19/19 PASS; buildPlanListTree 15/15 PASS; npx tsc --noEmit PASS; product git diff --check PASS; file-size gate PASS; pre-push cargo check/fmt/clippy PASS.
