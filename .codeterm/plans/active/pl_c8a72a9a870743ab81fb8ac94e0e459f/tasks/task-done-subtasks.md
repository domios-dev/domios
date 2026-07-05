---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_aee635de2dd0412b8ea5a0cb2a7ad612
handle: task-done-subtasks
title: Task not done while subtasks open — error on close
status: done
---

# CLI: a task is NOT done while it has open subtasks → error on close
Andrey: closing a task whose subtasks aren't all closed must be REJECTED at CLI with a clear error. Add to `plan task close` (and `task` close path): if the task has child tasks (parent_task_id) that are not done/cancelled, return an error listing the open subtasks + how to close them. Same guard in the service layer (not just CLI) so REST/socket also enforce. Test: close parent with open child → error; close after children closed → ok. Ties to cloud layout (a parent with open subtasks stays in_progress, pulls left).
