---
plan_id: pl_2e932e82ad614cdd9f08072480b0857e
kind: task
id: task_95984606dff445fe9ef540bff8731446
handle: ui-ux-iter11-float-the-swarm-onto-the-board-budget-bound-presence-pins
title: UI/UX iter11 — float the swarm onto the board (budget-bound presence pins)
status: done
---

Screen-space CanvasPresenceLayer pins each executing agent's AgentStack outside the task's top-right corner (Figma-multiplayer); click eases camera to task (+blink), hover=roster. LOD W0 dot / W1 avatars+ring / W2+ name flag. presenceOverlays.ts budget-bound (MAX_ACTIVE_PRESENCE_OVERLAYS=48, O(active), viewport-cull, priority-cap, memoized scene/transform/visibility -> zero per-frame O(N)). 5k-fixture frame check PASSED 4.3s count<=48. Pin outside corner -> card content clear (north-star#1). layout/scene memos cameraScale-decoupled. 0 overflow; tsc+presenceOverlays 10/10+cloudFlush 9/9+cards 20/20+presence-float 2/2. DATA DEP: live pane/agent-registry->buildTaskPresence (synthetic until wired). Committed feat/canvas-perf-polish (unpushed).
