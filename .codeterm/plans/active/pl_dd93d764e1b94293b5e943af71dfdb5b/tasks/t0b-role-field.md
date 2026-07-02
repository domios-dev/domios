---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_d52b73ff7a704f05a0377c06dd15b647
handle: t0b-role-field
title: 'Task executor-role field: model + CLI + doctor warning'
status: pending
---

Add a structured executor-role field to plan tasks so the Orchestrator routes agent personality per task declaratively (user requirement; today only prose role_guidance + runtime taken_by_role exist).

Steps:
1. Model: executor_role Option<String> on the task struct (src-tauri/src/service/plan/mod.rs) with serde + persistence parity (plan.json/body_files round-trip).
2. CLI: --role flag on plan task add + plan task update; loose validation (non-empty slug), vocabulary documented not enum-enforced.
3. Render: task show line, plan get markdown task bullets, plan state open-lanes entries include the role.
4. Doctor: quality WARNING (not error) when an execution-kind task lacks executor_role; planning-kind tasks exempt.
5. Docs: ORCHESTRATION doc quality-fields + Planner handback list; docs/contributing/agents.md as the vocabulary source (backend-dev, frontend-dev, code-reviewer, docs-sync, release, planner, qa); bump revision tags.
6. Tests: unit tests for set/update/render/doctor-warning.
7. Live verification: backfill executor_role across plan pl_dd93d764e1b94293b5e943af71dfdb5b (rust-backend: t0-planning-kind + this task; frontend-ux: t1-view-order, t2-detail-overlay, t2b-spec-pres, t3-board, t4-list, t5-canvas; qa-playwright: t6-e2e-gate; code-reviewer: t7-review; planner: planner-lane, t8-replan-loop) and paste the resulting plan state open-lanes lines in the report.

Commands: ./build.sh check; single narrowed cargo test run; ./build.sh cli; backfill via codeterm plan task update --role.
Report: field/API summary, doctor warning behavior, backfill evidence, doc diff note.
