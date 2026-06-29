---
tags:
- friction
---

Found by Curie writing the S1 verify spec. The backend canvas focus contract emits payload {task_id}, but the frontend CanvasView 'canvas-focus' event handler reads {node_ids, plan_ids}. Net: 'canvas focus --task <id>' does NOT recenter the camera on that task today. The epic's test strategy + S1 criterion rely on DETERMINISTIC focus-by-id, so this must be fixed for clean Playwright targeting (Curie's spec works around it by measuring geometry by data-task-id directly).