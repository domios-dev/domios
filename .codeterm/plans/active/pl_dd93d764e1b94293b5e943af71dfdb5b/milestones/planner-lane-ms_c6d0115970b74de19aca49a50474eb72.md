---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_c6d0115970b74de19aca49a50474eb72
handle: planner-lane-ms_c6d0115970b74de19aca49a50474eb72
title: status_update
---

PARTIAL: ROUND-2 REPLANNING + MECHANICS — the plan changed substantially, re-read before next dispatch.
FEEDBACK PROCESSED (user screenshots, loop iteration 1/3): Board multi-plan stacking is the core failure; cards leak worktree paths and raw STATUS UPDATE/DECISION text; column scroll clips cards; List still leads with the spec tree instead of tasks; Canvas zoom range wrong (1000% max, floor too shallow, world too sparse).
SPEC: round-2 blocks appended to Board/List/Canvas requirement sections. Board: ONE plan at a time + PlanSwitcher (activity-ordered, default = current orchestrator's active plan), viewport-height board with real in-column scroll, cards = title/status/executor/branch-chip + at most a tiny activity hint, NO paths, NO raw milestone text. List: task-first stream grouped by wave, spec tree removed (header chip -> overlay), active groups first. Canvas: zoom clamped 10-400%, dense per-section packing with measured bounds, LOD retuned, no dot-cloud.
NEW SPEC: loop-mechanics (iterations must be recordable/visible — ORACLE: this plan's own replanning loop shows count 0 after 2 real iterations, nothing calls record_iteration) + competitive-execution (user-opt-in: plan task compete --competitors N, concurrent attempts with isolated worktree lanes + per-attempt model/role overrides, per-pane report resolution, judge verdicts, winner integrates, losers -> cleanup debt, grouped nudge rendering).
TASKS: t9-board-v2 (task_2840fbfa, p1), t10-list-v2 (task_986b9765, p1, needs t9's PlanSwitcher), t11-canvas-v2 (task_e5310c37, p2) — all opus frontend; t12-loop-repair (task_8360c00d, p2), t13-competitive-exec (task_c4d48f0a, p2) — rust, one lane, t12 then t13. Waves w9-w12 added, spec-linked, strides recorded.
DISPATCH SUGGESTION: t9 first (t10 depends on it), t11 parallel; t12/t13 rust lane parallel; judge in_review queue first as usual.
