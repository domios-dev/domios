---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_ea4f725c96d4408181f77cd12afdcde1
handle: inject-orchestration-help-into-automatic-plan-heartbeats
title: Inject orchestration help into automatic plan heartbeats
status: done
---

Make the automatic owner heartbeat useful even when no manual focus/lanes were saved. The state-reconciliation prelude must explain that open plan work remains, that the pane is acting as the assigned orchestrator, and that `codeterm docs ORCHESTRATION` is the procedure source. It should include exact commands to read reports, inspect orchestrator status, reconcile, close or bounce tasks, and update the plan.

Acceptance: owner heartbeat/nudge text is self-sufficient for a cold agent; it mentions `codeterm docs ORCHESTRATION`; it does not require delayed self messages; it does not depend on textual keyword heuristics in plan body; existing manually learned heartbeat focus/lanes still render if present.

Test plan: extend heartbeat/reminder tests to assert docs command and open-work guidance appear in owner heartbeat injection; worker charter remains compact and worker-specific.

Correctness: automatic guidance from structured plan ownership/open-work state, with manual heartbeat additions only as extra overlays.
