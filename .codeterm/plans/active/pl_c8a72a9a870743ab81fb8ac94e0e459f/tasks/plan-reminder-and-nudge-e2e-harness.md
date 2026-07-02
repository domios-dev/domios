---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_b23505a7f95d4fa58c8630357446f1b1
handle: plan-reminder-and-nudge-e2e-harness
title: Plan reminder and nudge E2E harness
status: done
---

Build a deterministic reminder/nudge test path and fix behavior if needed. The important case: an Orchestrator pane that owns an active plan with open/in_review work must receive plan nudges even when the orchestrator is not directly assigned to a task. The nudge must be heartbeat-prepended and include exact recovery commands: codeterm agent reports --pane <pane>, codeterm orchestrator status --plan <plan> --pane <pane> --json, close accepted in_review, bounce incomplete work, update heartbeat, and read codeterm doc orchestration / codeterm docs ORCHESTRATION if the agent forgot the playbook. Worker task-binding nudges must still work and terminal/done task bindings must not nudge.
