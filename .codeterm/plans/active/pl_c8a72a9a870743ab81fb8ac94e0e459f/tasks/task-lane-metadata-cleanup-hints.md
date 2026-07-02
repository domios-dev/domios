---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_6cd5f8933d1c470785673012c8de906c
handle: task-lane-metadata-cleanup-hints
title: Task lane metadata and cleanup hints
status: done
---

Add first-class or CLI-visible lane metadata for worker tasks: pane id, workspace id/name, worktree path, branch name, base/destination, and cleanup status. Reports or task views should surface actionable hints after integration: verify, close/bounce task, close pane, remove worktree, delete branch, prune. Keep this low-hanging: start with task metadata fields or structured milestones plus CLI rendering, not a large orchestration rewrite.
