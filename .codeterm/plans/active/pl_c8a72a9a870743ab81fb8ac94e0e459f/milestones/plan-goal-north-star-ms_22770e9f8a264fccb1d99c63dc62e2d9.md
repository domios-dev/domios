---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_22770e9f8a264fccb1d99c63dc62e2d9
handle: plan-goal-north-star-ms_22770e9f8a264fccb1d99c63dc62e2d9
title: decision
---

Periodic reinjection: every plan retrieval and orchestrator heartbeat carries goal_summary + postulates. Add codeterm plan goal show <plan>, plan goal set, and plan get includes goal by default; delayed self-heartbeat text must include the active plan id, goal vector, current next actions, and the postulates checkpoint. Recommendation: daemon-side heartbeat template reads Plan.goal at send time so stale queued heartbeat text cannot drift.
