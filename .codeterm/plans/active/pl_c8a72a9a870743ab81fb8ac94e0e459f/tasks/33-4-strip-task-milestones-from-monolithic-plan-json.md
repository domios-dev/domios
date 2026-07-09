---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_0006b2d7e48c4637bd3138ab42499c7f
handle: 33-4-strip-task-milestones-from-monolithic-plan-json
title: §33.4 strip task milestones from monolithic plan JSON
status: done
---

Fourth bounded §33 storage slice. Build on the accepted stride-log sidecar dual-write/hydrate path.

Objective:
- Stop storing task `milestones` in the monolithic `<plan>.json` after save, because the sidecar `tasks/<handle>/strides.jsonl` is now the durable source for task milestones.
- Preserve backward compatibility with legacy JSON that still has inline milestones and no stride log: hydrate should still load those, then save should create the stride log and strip JSON.
- Preserve milestone text via the stride log. Do not rely on milestone markdown sidecars for recovering the list of milestones.

Strict non-goals:
- No full task/section/meta explode migration beyond task milestone stripping.
- No append-only mutation path, compaction, post-merge reindex, or schema rewrite.
- No deletion of existing milestone markdown sidecar behavior unless tests prove it is safe and necessary.

Acceptance:
- Tests prove `PlanStore::save` writes stride logs and the raw plan JSON has empty `tasks[].milestones`.
- Tests prove reload hydrates milestones from sidecar when JSON milestones are empty.
- Tests prove legacy inline-only milestones still load and then migrate to sidecar+stripped JSON on save.
