---
plan_id: pl_2e932e82ad614cdd9f08072480b0857e
kind: task
id: task_55b0ea7b5b0f4590a746ebf5042ca883
handle: ui-ux-iter14-agents-glide-between-tasks-figma-multiplayer-motion
title: UI/UX iter14 — agents glide between tasks (Figma-multiplayer motion)
status: done
---

presenceGlide.ts screen-space rAF controller (ease-out-cubic 520ms) on a dedicated glide-wrapper (React never touches it -> no camera fight, no scene/layout re-run). Retarget-safe mid-glide, reduced-motion=teleport, budget-bound (visible+active <=48, drops culled). presenceDemo.ts (canvas.presenceAnimateDemo=1) rotates synthetic swarm every 1.6s so circles visibly glide. CanvasView untouched (729, lean). 0 overflow during animation; tsc+cloudFlush 9/9+cards 20/20+presenceOverlays 10/10+presence-float+presence-glide pass; memos cameraScale-decoupled. DATA DEP: live agent->task transitions from pane/agent registry. Committed feat/canvas-perf-polish (unpushed).
