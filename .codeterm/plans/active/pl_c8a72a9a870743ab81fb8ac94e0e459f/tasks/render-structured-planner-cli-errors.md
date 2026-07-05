---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_6d79c39d4d2444979572ea95daccadaf
handle: render-structured-planner-cli-errors
title: Render all structured planner CLI errors
status: done
---

Implement friendly CLI renderers for structured planner errors found by audit. Scope: cli/src/cmd/plan_errors.rs and focused tests in cli/src/cmd/plan/tests.rs. Add renderers for bad_mode, bad_priority, context-aware not_found, context-aware bad_status, io, and a guard/test so every current PlanError::to_json type in src-tauri/src/service/plan/mod.rs has a non-raw renderer. Do not broaden scope. Keep messages actionable with exact commands. Known transition_denied renderer must keep passing. Run focused cli plan tests and git diff --check. Report in_review with findings/gates.
