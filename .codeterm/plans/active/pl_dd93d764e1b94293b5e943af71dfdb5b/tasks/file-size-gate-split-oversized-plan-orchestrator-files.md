---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_def1f314c9fc4890b93c8fd03b33e701
handle: file-size-gate-split-oversized-plan-orchestrator-files
title: 'File-size gate: split oversized plan/orchestrator files'
status: done
---

Current integration branch has release-blocking file-size violations: cli/src/cmd/plan_doctor.rs 1065, src-tauri/src/service/plan/mutations.rs 1020, src-tauri/src/service/orchestrator.rs 1026, src-tauri/src/socket/server/plan.rs 1003. Fix by splitting responsibilities into smaller files/modules. Do not add allowlist entries. Prefer moving cohesive helpers/tests/route handlers out of facade files. Keep all source files under 1000 lines and mod.rs-style facades under 800 where practical. Report with exact file line counts and gates.
