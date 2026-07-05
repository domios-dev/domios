---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: section
id: sec_e7f3bd47f5fb412182a55045c7fc8b42
handle: w0-planning-kind
title: Wave 0 — Planning-kind task support (CLI/backend)
---

Wave 0 (independent Rust/CLI lane; can run parallel to all frontend waves): first-class planning-kind tasks in the planner backend.

Scope:
- src-tauri/src/service/plan/mod.rs + mutations.rs: task-creation gate allows kind=="planning" while PlanMode::Planning without force; all other kinds keep TaskCreationRequiresApproval.
- Doctor/quality rules (src-tauri/src/service/plan/hints.rs, next_actions.rs): tasks_before_approval ignores planning-kind tasks; quality checks (missing body, criteria) still apply to them.
- CLI surface unchanged (--type planning already parses); help text for plan task add mentions the planning kind.
- Docs: docs/ORCHESTRATION.md and src-tauri/prompts/orchestration_toolset.md Planner Profile/planning-method updated; bump doc revision tags.
- Tests: unit tests beside the gate (service/plan tests modules) — planning-kind add succeeds in planning mode, execution-kind still blocked, doctor rule behavior both ways.

Verification: ./build.sh check; targeted cargo test (one run, narrowed filter, e.g. service::plan); ./build.sh cli + manual `codeterm plan task add --type planning` against a scratch planning-mode plan on dev.
Approval note: this wave changes orchestration contract text — Orchestrator should sanity-read the doc diff at review.
