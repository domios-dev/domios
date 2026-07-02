---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_fa9c2a8b2ca34c7b8f63b2117f9615a3
handle: 33-3-task-stride-log-sidecars-dual-write-and-hydrate-ms_fa9c2a8b2ca34c7b8f63b2117f9615a3
title: status_update
---

Accepted and integrated §33.3 task stride-log sidecars dual-write/hydrate.

Changes:
- Added task-scoped `tasks/<task-handle>/strides.jsonl` paths.
- `persist_bodies` now writes each task's current milestones to a deterministic folded JSONL sidecar while keeping inline `task.milestones` in plan JSON.
- `hydrate_and_migrate` reads existing stride logs, combines them with inline milestones, dedupes by id, and sorts by `(ts, id)`.
- INDEX/file manifest now include stride logs when a task has milestones or the sidecar exists.
- Existing long milestone markdown sidecars remain supported.

Orchestrator gates:
- cargo fmt --manifest-path src-tauri/Cargo.toml -- --check: passed.
- cargo test --manifest-path src-tauri/Cargo.toml --lib service::plan::body_file_tests -- --nocapture: 6 passed.
- cargo test --manifest-path src-tauri/Cargo.toml --lib service::plan::stride_log -- --nocapture: 7 passed.
- git diff --check: passed.
- bash scripts/check-file-sizes.sh: reports unrelated pre-existing oversized files; touched files are under hard limits.
- CODETERM_API_PORT=7686 ./build.sh check: passed.

Residual risk:
- This is dual-write/hydrate only. It intentionally does not strip milestones from JSON, implement append-only mutation, explode migration, compaction, or post-merge reindex.
