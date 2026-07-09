---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_d2a91c3db96a4d37a1572a7ec61f8850
handle: 33-6-watcher-reloads-task-stride-log-sidecar-changes
title: §33.6 watcher reloads task stride-log sidecar changes
status: done
---

Sixth bounded §33 storage slice after merge-preserving stride-log saves.

Problem:
- The plan watcher recursively watches `.codeterm/plans`, but `plan_id_from_path` only recognizes `.json` and `.md` files.
- `tasks/<task-handle>/strides.jsonl` changes from git merge=union or manual sidecar edits can be ignored, so the in-memory plan/search index may not hydrate the merged strides until some unrelated JSON/MD event occurs.

Objective:
- Teach the watcher path classifier to resolve task stride-log paths like `.codeterm/plans/<plan_id>/tasks/<task-handle>/strides.jsonl` back to `<plan_id>`.
- Teach project path resolution for those JSONL sidecars.
- Add focused unit tests for both path helpers.
- If small and clean, add a PlanStore-level test proving `reload_from_disk`/`scan_project` hydrates a sidecar-only appended stride after the JSON plan has stripped milestones.

Strict non-goals:
- No full explode/slim plan migration.
- No compaction.
- No filesystem debounce redesign.
- No frontend changes.

Acceptance:
- Watcher helper tests cover JSON plan, section/task markdown, and task stride-log JSONL paths.
- Existing body-file/stride-log tests still pass.
- `cargo test service::plan::watcher` and focused storage tests pass in hot checkout.
