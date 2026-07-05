---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_69893989106a4e019d3f5d1bcfb4fcf9
handle: audit-and-model-task-execution-attempt-metadata
title: Audit and model task execution-attempt metadata
status: done
---

Audit task metadata for orchestration continuity and define the execution-attempt schema. Identify every place that currently stores live assignment, lane metadata, reports, ledger rows, milestones, and status transitions. Produce and implement the minimal task-side schema needed to preserve executor history without keeping stale presence alive.

Required fields to consider: executor pane id, pane title, provider/agent type, model, session id, role, parent/orchestrator pane id, mesh peer, task prompt/task-file digest, branch, worktree, base/destination, report ids/statuses, changed files, test summary, assigned_at, reported_at, reviewed_at, closed_at, outcome, attempt number, retry/supersedes relation.
