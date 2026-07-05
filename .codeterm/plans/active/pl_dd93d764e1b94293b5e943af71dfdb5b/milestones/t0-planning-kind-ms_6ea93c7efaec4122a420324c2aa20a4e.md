---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_6ea93c7efaec4122a420324c2aa20a4e
handle: t0-planning-kind-ms_6ea93c7efaec4122a420324c2aa20a4e
title: status_update
---

IN_REVIEW: STATUS: in_review
CHANGES: committed 590707c5 on lane/t0-planning-kind (7 files):
- src-tauri/src/service/plan/mutations.rs: ensure_task_creation_allowed(plan_id, kind, force) — PlanMode::Planning + kind=="planning" => Ok (no --force); all other kinds keep TaskCreationRequiresApproval; Complete still gated.
- src-tauri/src/api/plan_routes/tasks.rs + src-tauri/src/socket/server/plan.rs: both task.add callers pass effective kind into the gate.
- cli/src/cmd/plan_doctor.rs: tasks_before_approval counts only non-planning tasks; planning-kind never trips it, execution-kind in planning mode still errors.
- tests: gate unit test (both dirs incl. planning) + 2 new doctor tests.
- docs/ORCHESTRATION.md (rev 15->16) + src-tauri/prompts/orchestration_toolset.md document planning kind.
Note: p0c externalized-body fix left untouched as instructed.

BUILD/TYPECHECK/TESTS:
- cargo check src-tauri --lib: Finished clean; cargo check cli: Finished clean.
- src-tauri: task_creation_requires_plan_approval_unless_forced ... ok (1 passed).
- cli: planning_kind_tasks_do_not_trip_approval_gate ... ok; execution_kind_task_in_planning_mode_trips_approval_gate ... ok.
- ./build.sh cli: installed to ~/.codeterm/bin + ~/.codeterm-dev/bin (EXIT=0).
- Manual doctor E2E with freshly-built release binary against scratch plans: planning-kind plan => 0 tasks_before_approval; implement-kind plan => 1 tasks_before_approval. Verified.

FOLLOWUP:
- Gate (daemon) verified via direct unit test + compile of both callers; I did NOT restart shared dev 7686 for a CLI round-trip because the plan-view-e2e-gate lane is actively using it. If you want a live daemon round-trip, restart dev from this worktree (or an isolated CODETERM_INSTANCE) and run: plan create (planning mode) -> plan task add --type planning (should succeed) -> plan task add (should be blocked).
- Out of scope, noted: 'plan batch' task creation bypasses ensure_task_creation_allowed entirely for all kinds (pre-existing); not touched.
