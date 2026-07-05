---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_b6f1f0cf50254157a4818227a43dba34
handle: bug-keep-current-agent-task-presence-accurate-on-canvas
title: 'BUG: keep current-agent task presence accurate on canvas'
status: done
tags:
- presence
- canvas
---

Investigate and fix stale/misleading canvas presence for the current agent: when an orchestrator/worker is attached to a plan task, the canvas should show the correct current task/plan focus and update when assignment changes.
