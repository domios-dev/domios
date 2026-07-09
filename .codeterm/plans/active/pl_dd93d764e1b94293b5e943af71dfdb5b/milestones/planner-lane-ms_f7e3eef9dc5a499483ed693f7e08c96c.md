---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_f7e3eef9dc5a499483ed693f7e08c96c
handle: planner-lane-ms_f7e3eef9dc5a499483ed693f7e08c96c
title: status_update
---

PARTIAL: Active-work visibility planned (user request).
DESIGN: one shared activeWork selector (planView/activeWork.ts over PlansProvider: in_progress/in_review + executor presence + freshness) feeding SIX surfaces — (1) PlanSwitcher per-plan in-flight badges (cross-plan visibility while one plan is on screen), (2) List pinned 'In flight' strip above the stream, (3) Canvas presence glow + fly-to-active control, (4) Sidebar WorkspacePlanEntry badge + tooltip, (5) Composer strip shows THIS pane's current task, (6) Board columns (existing). Stale in-flight (no stride/report activity, threshold shared with stale_in_review drift) renders visually distinct everywhere — forgotten work surfaces itself. Identical status colors + click-through to the shared detail overlay on all surfaces. No polling — derives from existing provider updates.
GRAPH: spec section active-work + wave w13-active-work + task t14-active-work (task_5c00ad82, opus, p2), spec-linked, stride recorded. DEPENDS on t9-board-v2 (PlanSwitcher) + t10-list-v2 (v2 stream) — dispatch t14 after those land.
STATE: 26 tasks, coverage 100%. Dispatch queue for you: t9 -> t10/t11 -> t14; t12->t13 rust lane independent.
