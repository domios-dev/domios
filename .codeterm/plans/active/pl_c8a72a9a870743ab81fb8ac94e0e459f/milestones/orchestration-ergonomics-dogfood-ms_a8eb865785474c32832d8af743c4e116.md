---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_a8eb865785474c32832d8af743c4e116
handle: orchestration-ergonomics-dogfood-ms_a8eb865785474c32832d8af743c4e116
title: review
---

ORCHESTRATOR FRICTION (lived, this session): WINS — plan record + batch $ref collapsed 6 calls→1 (huge); task-body-as-spec + thin launcher = clean delegation. FRICTION — (1) priority enum is p0-p3, not high/low — errored on first use, non-obvious; (2) CLI socket-discovery hijacked when a worker rebuilt the global CLI (fixed in 5efb622a, but cost time); (3) cross-daemon report (prod pane→dev plan) fails → had to use curl/REST; (4) no plan/task search → grepped JSON by hand (F1 fixes); (5) codeterm agent providers via CLI returns 401 (token not auto-attached); (6) agent spawn does not echo resolved provider+model → spawned wrong provider silently (task_242bcbba). Each is a real improvement target.
