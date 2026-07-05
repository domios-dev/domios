---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_de43b7d9efbc41c1a229276bd7546fc5
handle: t0-planning-kind
title: 'Planning-kind tasks: CLI/backend gating + doctor fix'
status: done
---

Make planning-kind tasks first-class in the planner backend, and fix the doctor body false-positive discovered during this planning session.

Steps:
1. Gate: in src-tauri/src/service/plan (mod.rs error PlanError::TaskCreationRequiresApproval + the task-add path in mutations.rs), allow task creation while PlanMode::Planning when the requested kind == "planning" (task.kind already exists as a free string via --type). All other kinds keep the approval requirement and the --force escape hatch unchanged.
2. Doctor: in the quality rules (src-tauri/src/service/plan/hints.rs / next_actions.rs), (a) tasks_before_approval ignores planning-kind tasks; (b) the "task is missing body" check must resolve externalized bodies — bodies are written to .codeterm/plans/active/<plan>/tasks/<handle>.md sidecars while plan.json inline body stays empty, so today EVERY task with a body-file trips a false positive (reproduced on this very plan). Read through the body_files layer, not raw plan.json.
3. Help/docs: plan task add help mentions --type planning; update docs/ORCHESTRATION.md and src-tauri/prompts/orchestration_toolset.md (Planner Profile + planning-method: a planning task may precede approval); bump doc revision tags.
4. Tests: unit tests beside the gate and doctor rules — planning-kind add in planning mode OK, execution-kind blocked, doctor both ways, externalized-body resolution.

Commands: ./build.sh check; single narrowed cargo test run (e.g. service::plan::); ./build.sh cli; manual: create scratch planning-mode plan on dev, verify add/doctor behavior, then delete it.
Report: gate diff summary, doctor rule changes, test output, doc diff note for Orchestrator contract review.
