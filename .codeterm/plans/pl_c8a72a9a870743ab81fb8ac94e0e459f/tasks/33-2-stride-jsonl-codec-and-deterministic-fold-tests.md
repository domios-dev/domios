Second bounded §33 storage slice after the merge=union spike. Do not implement the whole plan explode migration.

Objective:
- Add a small Rust module for append-only stride/event JSONL primitives under the plan service.
- It should serialize/deserialize task `Milestone` records as one JSON object per line, append atomically enough for single-writer-per-worktree, and fold a log deterministically by deduping `id` and sorting by `(ts, id)`.
- Keep it pure/small and testable. Do not wire it into PlanStore persistence yet unless the change stays obviously tiny and fully covered.

Acceptance:
- Focused Rust tests cover append/read, duplicate id dedup, deterministic `(ts, id)` fold ordering, malformed-line error behavior, and tempdir-only filesystem use.
- Source files remain under 1000 lines; `mod.rs` remains a light facade.
- No schema rewrite, no custom merge driver, no broad migration.
