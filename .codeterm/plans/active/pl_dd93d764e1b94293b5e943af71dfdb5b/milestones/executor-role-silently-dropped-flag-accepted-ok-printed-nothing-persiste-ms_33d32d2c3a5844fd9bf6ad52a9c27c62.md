---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_33d32d2c3a5844fd9bf6ad52a9c27c62
handle: executor-role-silently-dropped-flag-accepted-ok-printed-nothing-persiste-ms_33d32d2c3a5844fd9bf6ad52a9c27c62
title: status_update
---

STATUS: partial (code done, gates + live backfill pending)
BRANCH: lane/task-67d426a8-cli-state
SHA: 29023473afac30ad2867d1d871d923e6ac130fb2
WORKTREE: /Users/rollacode/Developer/codeterm-worktrees/task-67d426a8-cli-state

ROOT CAUSE (the exact dropping links): executor_role was NOT lost in the write path. plan.task.add RPC (plan_task_rpc.rs:294), update_task mutation (task_ops.rs:83), plan.json serialization, plan get --json, and the quality doctor ALL persist/read it correctly (verified live on dev: update --executor-role qa -> plan get --json shows qa; doctor fires missing_executor_role for a role-less task, silent for a role-set one). The field was dropped on TWO readback surfaces only:
  1. task sidecar .md frontmatter — body_files::persist_bodies built a fixed identity-only metadata map (plan_id/kind/id/handle/title/status/tags) that never included executor_role.
  2. CLI task show renderer — cli/src/cmd/task.rs never printed the field (task.show RPC serializes the full Task, so the data was present; only display was missing).

WHY DOCTOR WAS 'SILENT ON 28 LIVE TASKS': not a doctor-logic bug. Doctor reads the canonical plan.json (plan_doctor.rs:129), where the field is present, and fires correctly when absent (proven live). Likely live causes: the flagged tasks were planning-kind (exempt by design) and/or a stale non-canonical plan.json copy — the stray_sidecar check already exists for that. No code change needed in the doctor.

CHANGES:
  - src-tauri/src/service/plan/body_files.rs — emit executor_role into task frontmatter when set (trimmed, skip-if-empty).
  - cli/src/cmd/task.rs — render executor_role line in task show via pure executor_role_line() helper + unit tests.
  - src-tauri/src/service/plan/body_file_tests.rs — SECOND-REGRESSION LOCK: one cross-layer test asserts write -> plan.json -> sidecar frontmatter -> reload all carry the field (prior two tasks tested layers in isolation, which is how it broke twice).

TESTS RUN (narrow only): none executed — CODE-ONLY lane. Live behavioral repro done against the running dev daemon (non-build): confirmed persistence + doctor firing + frontmatter/task-show gaps. Repro plan pl_t1jrx0 created and deleted.

GATES REQUESTED (QA queue):
  - cargo test (narrow): service::plan::body_file_tests::executor_role_persists_across_json_frontmatter_and_reload ; cli executor_role_render_tests
  - cargo fmt --check (passed at lane push) + clippy on both crates
  - No tsc/Playwright needed (no TS/UI touched).

FOLLOWUP (out of code scope):
  - Backfill 28 open tasks per the Planner role-map stride — LIVE DATA mutation on the prod plan; needs orchestrator to run codeterm plan task update --executor-role per the map. Not done here (CODE-ONLY, and it's a prod-plan write).
  - Separate GAP (not this task's path): batch UpsertTask op (batch.rs BatchOp::UpsertTask) has no executor_role field at all, and the task.add inbox RPC (plan_task_rpc.rs:170) omits executor_role from NewTask. Neither is the reporter's path (plan task add/update both work), but flag if declarative role should flow through bulk/inbox creation too.
  - Overlap with stale lane/e40a28cc-cli-guidance: not merged; no reusable commits inspected as adjacent (my changes are self-contained).
