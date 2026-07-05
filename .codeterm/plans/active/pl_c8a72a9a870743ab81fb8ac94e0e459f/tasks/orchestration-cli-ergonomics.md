---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_e758327f72b14b2ab779033d363f4e96
handle: orchestration-cli-ergonomics
title: L2 orchestration CLI ergonomics for plan heartbeats
status: done
---

Fix heartbeat flag semantics: --clear-lanes --lane X must reject clearly or clear-then-set, never silently drop lanes. Replace stale Re-arm exactly one heartbeat wording with plan-owned heartbeat language in prompts/tests. Hints must include exact live commands for report/reconcile/close/bounce/cleanup. Add orchestrator status JSON with reports, live_lanes, drift, and next_actions.
