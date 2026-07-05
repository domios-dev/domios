---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_b06247dbe1824c91b2e15760c5801157
handle: implement-strict-planning-lifecycle-and-task-creation-gate
title: Implement strict planning lifecycle and task-creation gate
status: done
---

Implement the strict planning lifecycle model from plan.md directly in CodeTerm.

Scope:
- Add plan mode/state for planning, approved, execution, complete.
- New plans start in planning.
- Task creation refuses planning/complete plans unless the caller explicitly forces a draft task.
- CLI error must explain how to finish planning: read docs ORCHESTRATION, run plan state, run plan doctor --kind quality, approve plan, retry task add.
- Heartbeat runtime state is rejected for planning-only notes.
- No legacy effective-mode fallback.

Acceptance:
- task add is blocked before approval with actionable CLI text.
- force remains explicit and visible.
- heartbeat refuses runtime execution state in planning mode.
- next-actions/state expose approval readiness.

Gates already run locally:
- cargo test --manifest-path cli/Cargo.toml plan_planning_gate_tests -- --nocapture PASS
- cargo test --manifest-path cli/Cargo.toml plan_doctor -- --nocapture PASS
- cargo test --manifest-path src-tauri/Cargo.toml service::plan::heartbeat_tests --lib -- --nocapture PASS
- cargo test --manifest-path src-tauri/Cargo.toml service::plan::next_actions --lib -- --nocapture PASS
- cargo test --manifest-path src-tauri/Cargo.toml service::plan::state --lib -- --nocapture PASS
- CODETERM_API_PORT=7686 ./build.sh check PASS
