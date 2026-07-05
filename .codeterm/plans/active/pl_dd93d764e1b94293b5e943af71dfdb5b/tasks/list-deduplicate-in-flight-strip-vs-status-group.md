---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_3e9424054eca48aa93232eb5a28bbaf6
handle: list-deduplicate-in-flight-strip-vs-status-group
title: 'List: deduplicate In-flight strip vs status group'
status: done
---

## What

List view renders the same in-flight tasks twice back-to-back: the pinned "IN FLIGHT" strip (from Active-work surfacing, task_5c00ad82 — designed to keep in-flight visible regardless of filters) sits directly above the "IN PROGRESS + REVIEW" status group (from List v4 flat grouping, task_168db0b7) which shows the identical rows. Two shipped features colliding — screenshot evidence in round-7c feedback.

## Fix (keep the strip's purpose, kill the redundancy)

The strip's reason to exist is visibility when the stream would HIDE in-flight work. Render it conditionally:
- SHOW the strip when the current view state hides any in-flight task from the stream: active status filter (Done/Cancelled/etc), text filter excluding them, or the in-progress group collapsed.
- HIDE the strip when the In Progress + Review group is expanded, unfiltered, and therefore already the first thing on screen.
- Rule implemented as a pure predicate (stripVisible(viewState, inflightTasks)) with unit tests; no polling, derives from existing state.
- The strip keeps its click-through and freshness display unchanged when shown.

## Files

PlanListView + its strip/grouping modules (buildPlanListTree / activeWork wiring), component tests.
