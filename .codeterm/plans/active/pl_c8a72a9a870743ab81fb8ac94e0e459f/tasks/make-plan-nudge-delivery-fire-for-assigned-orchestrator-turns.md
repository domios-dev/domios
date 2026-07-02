---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_32be8728fd0c42f8a0dc3bf5f0f65b24
handle: make-plan-nudge-delivery-fire-for-assigned-orchestrator-turns
title: Make plan nudge delivery fire for assigned orchestrator turns
status: done
---

Fix the delivery trigger, not just eligibility/rendering. Current inspection shows maybe_send_plan_reminder is only called from agent report submission, so an assigned orchestrator may not receive automatic plan reminders during normal turns even when it owns an execution-mode plan with open tasks. Find the correct messenger/chat/pane lifecycle hook and make plan-owned nudges fire for the assigned orchestrator while unfinished execution work exists, without delayed self-sends and without spam. Preserve worker report nudges until report/in_review.

Acceptance: a dev/live smoke can prove an owner pane receives the state-reconciliation heartbeat/nudge without manually scheduling a delayed send; worker behavior remains report-pending only; nudge counter still throttles by config; no prose heuristics.

Test plan: focused Rust tests for trigger path if available, plus dev smoke instructions against 7686.

Correctness: automatic delivery from structured plan owner/open-work state, not from chat text or manual heartbeat setup.
