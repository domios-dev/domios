---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_9dc8f93d6f374118bb870e917fd1b7a4
handle: state-integrity-39-7-task-execution-binding-metadata-git-branch-agent-pane-provider-working-folder-worktree-on-each-task-for-find-restore
title: 'State-integrity §39.7: task execution-binding metadata — git BRANCH + AGENT (pane/provider) + working FOLDER (worktree) on each task, for find/restore'
status: done
tags:
- state-integrity
---

Task model only has model+parent_task_id — no record of WHERE/WHO works it. Add execution-binding fields: git branch, agent (pane id + provider), working folder (worktree path); set at assign/dispatch + on worker report; show in 'plan get'/§39.6 view + on the canvas card. So an orchestrator (or a recovering one) can instantly find the branch/agent/folder for any task = faster integration, restore, sweep. Ties to §39.3 (task<->pane lifecycle) — lifecycle events update these.
