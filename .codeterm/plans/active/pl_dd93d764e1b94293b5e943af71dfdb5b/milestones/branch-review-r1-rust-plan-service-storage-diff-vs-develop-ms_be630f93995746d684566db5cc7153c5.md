---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_be630f93995746d684566db5cc7153c5
handle: branch-review-r1-rust-plan-service-storage-diff-vs-develop-ms_be630f93995746d684566db5cc7153c5
title: review
---

HIGH: src-tauri/src/service/plan/body_files.rs:645 rewrites each task's strides.jsonl during every plan save. Current behavior: persist_stride_log reads the existing log, folds it with in-memory milestones, then rewrite_milestones atomically renames a complete replacement file. A second daemon/worker/manual sidecar append that lands after the read and before the rename is silently discarded by the rename; the readback verifier only checks the current mutation in memory, so it will not detect losing someone else's appended report/stride. This still allows plan history data loss under concurrent report/save traffic. Concrete fix: make stride writes append-only for new milestones and avoid rewriting the shared log on ordinary saves, or guard all read-merge-rewrite paths with an interprocess per-plan/task file lock and retry if mtime/size changed; add a regression test that races persist_stride_log/save against an external append and asserts both milestones survive.
