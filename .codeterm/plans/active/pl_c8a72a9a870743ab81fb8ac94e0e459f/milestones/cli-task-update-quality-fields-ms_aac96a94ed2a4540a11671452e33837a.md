---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_aac96a94ed2a4540a11671452e33837a
handle: cli-task-update-quality-fields-ms_aac96a94ed2a4540a11671452e33837a
title: Implemented locally in hot checkout
---

task update now accepts --criterion, --test-plan, and --correctness; CLI forwards quality fields; backend plan.task.update persists acceptance_criteria/test_plan/correctness through update_task_quality. Verified cargo fmt, CLI parser tests, existing backend quality persistence test, ./build.sh cli, cli-manifest --write, ./build.sh check, git diff --check. Live smoke deferred until safe dev restart because current worker panes are active.
