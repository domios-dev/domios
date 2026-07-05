---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_3388d0473dd34e66bacc76c335f56a9d
handle: add-cli-api-respawn-from-task-history
title: Add CLI/API respawn-from-task-history
status: done
---

Add CLI/API ergonomics to inspect executor history and respawn/reopen work from a previous executor attempt. The respawn command should build a task prompt from prior context: original task body, last report summary, branch/worktree hints, model/provider/session metadata when useful, and exact report command. It must create a new live assignment while keeping the old attempt historical.
