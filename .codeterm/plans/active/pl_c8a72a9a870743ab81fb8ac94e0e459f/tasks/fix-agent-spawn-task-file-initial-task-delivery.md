---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_7021fa3b39f64bb4b2bdf10f86f03975
handle: fix-agent-spawn-task-file-initial-task-delivery
title: Fix agent spawn --task-file initial task delivery
status: done
tags:
- orchestration
- agent-spawn
---

Dogfood smoke on local project pl_130a773127ee4e9eaf4462764236ddb0 showed agent spawn created Codex worker panes and they reached ready state, but the --task-file payload was not injected. Pane text showed default Codex starter prompts; pane chat was empty; files/reports were absent until orchestrator manually ran codeterm send with the same task file. Investigate spawn readiness/task injection path and add a regression test that --task-file reaches Codex after agent ready.
