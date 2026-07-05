---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_e5d117ec75114a749455df279d5cb476
handle: 33-5-preserve-merged-stride-log-entries-on-save
title: §33.5 preserve merged stride-log entries on save
status: done
---

Fifth bounded §33 storage slice after task stride sidecars and JSON stripping.

Problem:
- `persist_stride_log(plan, task)` currently rewrites `tasks/<handle>/strides.jsonl` from the in-memory task milestones.
- After git merge=union or manual sidecar edits, a save from a stale/incomplete in-memory plan can truncate already-present stride log lines that were not hydrated into memory first.

Objective:
- Make stride-log persistence merge-preserving: before rewriting/folding a task stride log, read any existing `strides.jsonl`, combine it with the task's current milestones, fold/dedupe deterministically, and write the folded result.
- Keep raw plan JSON milestone arrays stripped as implemented in §33.4.
- Keep legacy inline-only migration working.

Strict non-goals:
- No full explode/slim plan migration.
- No append-only mutation API rewrite beyond preserving existing sidecar entries during save.
- No compaction or post-merge watcher/index changes.

Acceptance:
- Focused test proves an existing sidecar-only milestone survives a later `PlanStore::save` where the in-memory task has a different/new milestone.
- Focused test proves raw plan JSON still has empty `tasks[].milestones` after that save.
- Existing `body_file_tests` and `stride_log` tests pass in hot checkout.
