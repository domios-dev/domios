---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_540e7afaa133456a937019925adf159d
handle: lane-auto-cleanup-merged-branches-and-worktrees-retire-on-task-close
title: 'Lane auto-cleanup: merged branches and worktrees retire on task close'
status: pending
---

POST-RELEASE automation of the lifecycle standard's cleanup contract (depends on the naming standard lane/<task-shortid>-* and plan lanes liveness). On task close (done/cancelled) the daemon checks the task's recorded branch: if merged into the integration head -> delete the branch, git worktree remove its worktree if recorded and clean, record a cleanup stride; if NOT merged -> no deletion, flag 'unmerged lane on closed task' into cleanup-debt reminders (a human/orchestrator verdict, never auto-delete unmerged work). Config [orchestration.lane_cleanup] enabled/dry_run (dry_run logs what it would do — ship ON with dry_run first release). Safety: never touch branches without the task short-id in the name; never remove dirty worktrees; canonical checkout untouchable; every action is a stride.
