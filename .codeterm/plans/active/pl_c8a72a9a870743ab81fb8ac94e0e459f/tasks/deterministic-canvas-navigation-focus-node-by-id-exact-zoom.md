---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_3f409451008e4617add382cb851a7f58
handle: deterministic-canvas-navigation-focus-node-by-id-exact-zoom
title: 'Deterministic canvas navigation: focus node by id @ exact zoom'
status: done
---

Make 'go to node id, centered, at a chosen zoom' a real product method.

DONE (frontend, hot-reloaded): CanvasView canvas-focus listener now reads the real contract {task_id, plan_id, pane_id} (task/plan node id == its id; pane -> its assigned tasks) and an optional `scale`; added centerCameraOnNodes(ids, scale) for explicit-zoom centering vs fit-to-bbox; focusOnNodes(ids, scale?) threads it. Verified: POST /api/canvas/focus {task_id} centers + mounts + blinks the card (offset within 25% of viewport center, scale auto-fit 3).

TODO: (1) backend scale passthrough — add `scale` to CanvasFocusBody (REST) + socket canvas.focus payload + CLI `canvas focus --scale`; (2) canvas-inspect helper focusTaskById(page, id, scale) replacing pan/zoom mount heuristics; (3) rewrite @canvas-worlds-v3 to mount the probe via focus-by-id and run GREEN spanning W1/W2/W3.
