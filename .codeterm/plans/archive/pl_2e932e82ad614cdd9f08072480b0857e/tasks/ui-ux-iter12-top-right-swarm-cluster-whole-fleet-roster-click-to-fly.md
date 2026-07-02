---
plan_id: pl_2e932e82ad614cdd9f08072480b0857e
kind: task
id: task_22791c5a306c4507a0eebae301b8d6e6
handle: ui-ux-iter12-top-right-swarm-cluster-whole-fleet-roster-click-to-fly
title: UI/UX iter12 — top-right swarm cluster (whole-fleet roster + click-to-fly)
status: done
---

Viewport-anchored top-right cluster: whole active swarm as identity circles (screen-space, zoom-independent, camera-decoupled). One circle per distinct agent (deduped, current=live task). Collapsed pill: live pip + N working/idle + overlapping faces(cap5) + +N + chevron; click face -> ease-fly to task + blink. Expand -> roster: circle/handle/role/->task/branch. swarmRoster.ts budget-capped MAX_SWARM_ROSTER=32 + +N more (300-fleet bounded). Empty hides. Reuses AgentCircle+identity+focusOnNodes. 0 overflow real+300-fleet; tsc+swarmRoster 16/16+cloudFlush 9/9+presenceOverlays 10/10+cards 20/20+swarm-cluster 2/2; memos cameraScale-decoupled. Committed feat/canvas-perf-polish (unpushed).
