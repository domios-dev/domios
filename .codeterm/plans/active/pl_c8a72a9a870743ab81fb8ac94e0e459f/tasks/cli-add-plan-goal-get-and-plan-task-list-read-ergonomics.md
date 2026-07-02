---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_90f01ec50a634ab88998487d2a11f1b2
handle: cli-add-plan-goal-get-and-plan-task-list-read-ergonomics
title: 'CLI: add ''plan goal get'' and ''plan task list'' (read ergonomics)'
status: done
tags:
- friction
---

'codeterm plan goal' only has 'set' (no get) — must read goal out of 'plan get' markdown. 'plan task' has no 'list' — tasks only visible via 'plan get' md/json. Add 'plan goal get <plan>' and 'plan task list <plan> [--status]' for quick orchestrator reads.
