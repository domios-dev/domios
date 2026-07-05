---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: section
id: sec_583167108c604fcdb8b2b95e9c59f8e5
handle: w4-list
title: Wave 4 — List redesign
---

Wave 4: List redesign — secondary view. Depends on Wave 2 (shared overlay) and Wave 3's PlanContextHeader.

Scope:
- Interactive rows: task rows open PlanDetailOverlay; section rows toggle collapse; root rows open plan/spec detail.
- Collapse model in buildPlanListTree.ts (or a sibling listViewState.ts): expand-state map, done-only sections default collapsed, "n done" rollup rows.
- Quiet done styling; status filter chips + text filter in a thin toolbar; sticky plan header via PlanContextHeader.
- Keep density: one row per item, no cards.

File ownership: planView/PlanListView.tsx, planView/buildPlanListTree.ts (+ new listViewState.ts if cleaner), tests PlanListView.test.tsx + buildPlanListTree.test.ts, globals.css if list gets classes.
Verification: npm run test:unit; testids for e2e: plan-list-row click behavior, plan-list-collapse-toggle, plan-list-filter.
