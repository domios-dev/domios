---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_79f0de5336584b4db512a4136e19d397
handle: loop-compete-badges-on-task-cards
title: Loop + compete badges on task cards
status: done
---

## What

Tasks driven by a self-improving loop (`LoopSpec` in plan payload: target, budget, count, stagnation, status) carry no visual hint of looping. Add a loop iteration badge wherever the task is shown, visible when the task loops more than once.

## Design

- **Badge.** A small chip with a loop glyph + "xN" (N = LoopSpec.count) shown when a loop targets this task and count >= 2. Muted by default; if `count >= budget` or loop status is stagnating/exhausted, tint warning (existing warning tokens). Tooltip: "Loop <target-or-id>: iteration N of <budget> - <status>" + stop_when when set.
- **Surfaces.** Board card meta row, List row chips, task detail overlay/slide-over header (next to status pill). Reuse ONE shared component.
- **Resolution.** Loops live at plan level (`plan.loops[]`, target references the task). Resolve via a small pure helper `loopForTask(plan, taskId)` (unit-tested) shared by all surfaces; handle multiple loops targeting one task (show the active one; count badge aggregates if several active).
- **Click-through.** Clicking the badge opens task detail scrolled to loop info (or shows the tooltip content in the detail's Activity section header if detail-anchor is trivial; do not build a new loop panel).

Coordinate with the activity-log task (task_6bfdaa7125e643acb061f2282b0056b0): loop iterations recorded as working points should be visible in the same timeline.

## Round-6 addition: competitive marking chip

Tasks marked competitive at planning time (competition meta from task_c4d48f0adfb841c0849ac095e4690852 step 0: desired_attempts N) get a second chip from the same shared component family: distinct glyph + "xN" (visually distinct from the loop chip — different glyph/tint, same size grammar). Shown on board cards, list rows, detail header whenever the meta is present (pending AND during execution). During live competition the tooltip adds per-attempt state (n live / judging). Resolve via a pure helper over the task's competition meta — no inference. If the competition model has not landed yet, ship loop-only and record the gap in the report.
