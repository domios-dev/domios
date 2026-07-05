---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_168db0b723d04d72a0e5007d3d9464e1
handle: t16-list-v4
title: 'List v4: flat status groups + belonging chips'
status: done
---

List v4 — flat status-grouped list (round 4; supersedes the wave-grouping shipped in v3; the v3 alignment grid/typography/rhythm STAND). Read the ROUND 4 block of the List spec section first.

Steps:
1. buildPlanListTree -> flat model: status groups (In progress+In review / Blocked / Pending / Done-collapsed / Parked-folded once w15 lands — degrade gracefully before); within group order by activity freshness; slim sticky group headers with counts.
2. Belonging chip per row (plan section/wave name, dimmed): click -> plan document at that section (route exists after t18; before that, chip renders inert with tooltip).
3. Keep alignment grid columns, filters, active-work strip hook (t14).
4. Remove wave group headers entirely; verify no task becomes unreachable (chips carry belonging).
5. Tests: flat model unit tests; Playwright: order by status, chip presence, done collapse; before/after fixture screenshots.
