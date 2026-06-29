Third bounded §33 storage slice. Build on the accepted `stride_log` primitive; do not remove milestones from the monolithic plan JSON yet.

Objective:
- Add task-scoped `strides.jsonl` sidecar paths, e.g. `.codeterm/plans/<plan>/tasks/<task-handle>/strides.jsonl`, matching the existing `.gitattributes` merge=union rule.
- During `persist_bodies`, dual-write each task's current `milestones` into that JSONL log using the `stride_log` primitive. It is acceptable for now to rewrite/dedup deterministically rather than append on every save, as long as the result is deterministic and merge-safe for the next slice.
- During `hydrate_and_migrate`, if a task stride log exists, read it, combine with any inline milestones already in the plan JSON, fold/dedupe by id, and sort by `(ts, id)`.
- Add manifest/INDEX entries for stride logs if they exist or if the task has milestones.

Strict non-goals:
- Do not strip `task.milestones` from the JSON yet.
- Do not implement full explode migration, compaction, post-merge reindex, or schema changes.
- Do not alter milestone markdown sidecar behavior except as needed to coexist.

Acceptance:
- Tests prove save writes a task `strides.jsonl`, reload hydrates from it, inline+jsonl duplicates dedupe deterministically, and file_manifest/INDEX list it.
- Existing body-file tests still pass.
