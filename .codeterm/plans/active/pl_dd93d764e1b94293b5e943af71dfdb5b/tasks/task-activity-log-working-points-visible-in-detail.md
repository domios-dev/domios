---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_6bfdaa7125e643acb061f2282b0056b0
handle: task-activity-log-working-points-visible-in-detail
title: 'Task activity log: working points visible in detail'
status: done
---

## What

Working points (task milestones) and execution attempts exist in the model (`PlanTask.milestones`, `PlanTask.execution_attempts` — written by `plan record`, `agent report`, status changes) but are invisible: opening a task shows no history of what was done. Surface a task **activity log** in every task detail surface, and a plan-level trail in the plan document view.

## Design

**Task detail (primary).** In the shared detail surface (PlanDetailOverlay, and the upcoming slide-over task_f98cc5ef604f456591e74ebede558bf0 — coordinate, same component must serve both) add an "Activity" section: reverse-chronological timeline of milestones — kind chip (colored via milestoneColors.ts), text, relative timestamp, author (pane label when known). Execution attempts render as grouped entries (attempt N: pane, started/ended, outcome). Collapsed by default to the latest ~5 entries with "show all (N)"; full list scrolls inside the panel, never grows the overlay unbounded.

**Plan level.** Plan document view (task_8be0a8a58f5342659e2de4a756650daf — coordinate) gets a compact "Recent activity" rail: last N working points across all tasks (task title + kind + age), each click-through to the task detail. If the plan document view has not landed, ship the task-detail part alone — do not block.

**Data.** Everything is already in the plan payload the frontend receives — no new endpoints; verify milestones/execution_attempts survive the PlansProvider mapping (add fields to the TS types if the mapping drops them today).

**Style.** Quiet timeline: thin left time-rail, tiny muted type, kind chips using existing milestone colors. Matches the plan-view aesthetic; no cards-within-cards.
