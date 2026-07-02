---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_fcc769d160eb4bd08c29c3dfc45eb2db
handle: heartbeat-dogfood-worker-wait
title: Dogfood automatic heartbeat delivery with waiting worker
status: done
tags:
- 1.6.2
- followup
---

Spawn one Worker, assign this task, and instruct it to read the task then wait silently until it receives the automatic CodeTerm plan reminder/heartbeat. After the reminder arrives, it must run codeterm agent report with status in_review and quote the reminder marker it observed. The Orchestrator also waits for owner heartbeat on the same open plan.