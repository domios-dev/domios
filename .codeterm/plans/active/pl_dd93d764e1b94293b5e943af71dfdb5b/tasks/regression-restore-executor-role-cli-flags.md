---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_287dcb44dadd46f19d38c85ed23aca2f
handle: regression-restore-executor-role-cli-flags
title: 'Regression: restore executor-role CLI flags'
status: done
---

Regression discovered during round-7 planning: codeterm plan task add/update no longer expose --executor-role, even though Task.executor_role exists and task cards/doctor rely on the declarative executor-role field. Restore the CLI add/update flags and RPC parameter plumbing without regressing parser performance. Do not reintroduce the competitive CLI shape that made PlanTaskAction command construction hang. Keep the change scoped to executor_role.
