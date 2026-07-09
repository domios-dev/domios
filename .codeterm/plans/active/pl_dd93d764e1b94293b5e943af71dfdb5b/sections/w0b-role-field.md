---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: section
id: sec_653cc635579c433fae71b7c94ed36889
handle: w0b-role-field
title: Wave 0b — Task executor-role field (CLI/backend)
---

Wave 0b (Rust/CLI lane, parallel with Wave 0, likely same worker or sequenced right after t0-planning-kind): structured executor-role field on tasks.

Scope:
- Task model: add executor_role (Option<String>) to the task struct + serde in src-tauri/src/service/plan/mod.rs, persisted through body_files/plan.json like other scalar fields.
- CLI: --role on plan task add / plan task update; render in task show, plan get markdown, and plan state open-lanes lines.
- Doctor: quality warning (not error) for execution-kind tasks missing executor_role.
- Docs: ORCHESTRATION doc (task quality fields, Planner Profile handback list) + docs/contributing/agents.md (role vocabulary is the source list); bump revision tags.
- Tests: unit tests for set/update/render + doctor warning.
- Live verification: backfill executor_role on all tasks of plan pl_dd93d764e1b94293b5e943af71dfdb5b (rust-backend for t0/t0b, frontend-ux for t1/t2/t2b/t3/t4/t5, qa-playwright for t6, code-reviewer for t7, planner for planner-lane/t8).

Verification: ./build.sh check; narrowed cargo test (single run); ./build.sh cli; backfill commands against dev + `plan state` visual confirmation.
