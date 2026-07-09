---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_7089039cb16146c2a7aa78353848d642
handle: plan-index-mirror-false-failures-regression-of-task-354c755f-fix
title: 'plan_index_mirror false-failures: regression of task_354c755f fix'
status: done
---

VERIFY-FIRST REGRESSION: task_354c755f ('plan_index_mirror returns false failure after committed writes') is marked done, but the failure recurred twice since: (1) the creation-matrix QA lane reported 'plan_index_mirror is pending false-fail on update' under load (its report explicitly cited the task_354c755f class); (2) during Phase C the Orchestrator observed 'plan status updates are committing despite mirror lag, so I'm not retrying blindly' — i.e. the CLI still reports failure for writes that committed. Reproduce under load (the trigger both times): parallel plan writes while the watcher reconciles (the burst-coalescing path in service/plan/watcher.rs); establish whether the original fix missed a path (update vs add), regressed, or the mirror-pending state needs to be treated as success-with-lag in more call sites. FIX so a committed write NEVER reports failure; mirror lag is at most an informational note. Add a load test: N parallel task updates during watcher bursts -> zero false failures.
