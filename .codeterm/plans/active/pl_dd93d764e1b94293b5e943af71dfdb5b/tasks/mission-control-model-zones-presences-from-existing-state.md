---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_6c823ffa170b4cb09864dc496ec18620
handle: mission-control-model-zones-presences-from-existing-state
title: 'Mission control model: zones + presences from existing state'
status: done
---

## What

The data spine for mission control: a pure missionControl model module deriving every zone's content from EXISTING state — no new endpoints, no polling. Buildable in parallel with the design gate (model is layout-agnostic).

## Derivations (all from current payloads/selectors)

- Dispatch frontier: pending tasks, deps satisfied, not parked/blocked (reuse frontier logic semantics from next_actions if exposed; else derive).
- Work floor bays: in_progress tasks with lane/pane bindings + live pane statuses (activeWork selector + PaneStatusTracker statuses).
- QA bay: review_stage=qa tasks with qa_state queued|running order (QA pipeline task_49f03e58 — degrade gracefully to empty zone if unlanded).
- Gate desk: in_review (stage Review) tasks awaiting orchestrator judgment.
- Done shelf: done count + last N titles. Parked/blocked side rail.
- Agent presences: pane id -> {avatar meta, station, status} for orchestrator ref, planner ref (when landed), assigned workers, QA claimant.
- Event feed: last N milestones across tasks (reuse activity-log data path).

## Contract

`buildMissionControl(plan, statuses, activeWork): MissionControlModel` — pure, unit-tested per zone with fixtures including: empty plan, 6-lane busy plan, QA queue, competition (multiple attempts on one task -> multiple avatars on one bay), dead pane on a lane (presence shows Dead).
