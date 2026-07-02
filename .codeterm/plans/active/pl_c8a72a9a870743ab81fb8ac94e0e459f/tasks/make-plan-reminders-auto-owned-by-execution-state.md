---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_81a53f3364ab4799b442c7b879658814
handle: make-plan-reminders-auto-owned-by-execution-state
title: Make plan reminders auto-owned by execution state
status: done
---

Implement automatic plan reminder eligibility from structured plan state. Owner/orchestrator nudges only for active execution plans with non-terminal work; worker nudges only while the worker has a live assigned task that has not reported into review. The injected text must remind the owner that open work remains, explain where to re-read orchestration procedure with `codeterm docs ORCHESTRATION`, and avoid delayed self-send heartbeats.

Acceptance: no keyword heuristics; eligibility uses PlanMode and TaskStatus enums. Planning-mode plans with tasks do not nudge as execution. Execution-mode owner with pending/in_review work nudges. Worker with assigned in_progress task nudges. Worker with in_review/done/cancelled task does not nudge. Nudge text points to docs ORCHESTRATION and reports/status/reconcile commands.

Test plan: add focused Rust tests in service::plan::reminder_tests and service::plan::reminders. Run focused reminder/heartbeat tests plus cargo check/build.sh check.

Correctness: structured status/model based reminders, no prose parsing, no manual heartbeat prerequisite.