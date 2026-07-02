---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_dbb511f69bef4110825e56db2c527dd2
handle: bug-post-api-canvas-focus-emits-task-id-but-canvasview-canvas-focus-listener-reads-node-ids-plan-ids-focus-by-id-never-recenters-on-a-task
title: 'BUG: POST /api/canvas/focus emits {task_id} but CanvasView ''canvas-focus'' listener reads {node_ids,plan_ids} — focus-by-id never recenters on a task'
status: done
tags:
- friction
---

Found by Curie writing the S1 verify spec. The backend canvas focus contract emits payload {task_id}, but the frontend CanvasView 'canvas-focus' event handler reads {node_ids, plan_ids}. Net: 'canvas focus --task <id>' does NOT recenter the camera on that task today. The epic's test strategy + S1 criterion rely on DETERMINISTIC focus-by-id, so this must be fixed for clean Playwright targeting (Curie's spec works around it by measuring geometry by data-task-id directly).
