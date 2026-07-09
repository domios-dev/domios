---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_b9ba51dc00e746e6b01e97aaf114ce92
handle: mesh-agent-report-homes-to-the-task-s-origin-plan-ms_b9ba51dc00e746e6b01e97aaf114ce92
title: status_update
---

STATUS: in_review
CHANGES: src-tauri/src/service/hierarchy.rs, src-tauri/src/service/hierarchy/tests.rs — sha 460ef1783384063f53397f4412194c1b4762904a
BUILD/TYPECHECK/TESTS: not run (machine constraint, worktree build prohibited); logic verified by inspection
- submit_report homing block replaced with is_explicit_plan_foreign() guard
- relay_plan_batch_to_peer returns Result<(), String>, error surfaces in notices
- all brace/indent structure manually verified; no TS files touched
FOLLOWUP: orchestrator should run cargo test service::hierarchy on the warm build; specifically confirm the 6 new is_explicit_plan_foreign tests and the existing stale_plan_falls_through_to_binding test still pass (the fallthrough is preserved for local workers — only suppressed when foreign=true)
