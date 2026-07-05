---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_64c91cce0a5947f89644023ac3d7747a
handle: fix-plan-storage-prevent-stride-log-rewrite-races-from-dropping-concurre
title: 'fix(plan-storage): prevent stride log rewrite races from dropping concurrent appends'
status: done
---

Review R1 found a HIGH data-loss race. Current behavior: src-tauri/src/service/plan/body_files.rs:645 persist_stride_log clones task.milestones, extends them with read_milestones(&path), folds, then calls rewrite_milestones at src-tauri/src/service/plan/body_files.rs:653. rewrite_milestones in src-tauri/src/service/plan/stride_log.rs:58 writes a complete replacement via atomic rename. Because PlanStore::write_atomic at src-tauri/src/service/plan/mod.rs:120 calls body_files::persist_bodies for every save, any normal plan mutation can read an old strides.jsonl snapshot and later rename it over an external append from another daemon/worker/manual sidecar writer. Desired behavior: ordinary saves must not replace a shared JSONL log with a stale snapshot. Prefer append-only writes for newly created milestones plus deterministic hydration/folding on read; if compaction/rewrite is still needed, protect it with an interprocess per-plan/task lock and retry on observed mtime/size change. Test: create a regression that forces/readies the race window or directly exercises the chosen concurrency primitive so an append between read and rewrite cannot be lost.
