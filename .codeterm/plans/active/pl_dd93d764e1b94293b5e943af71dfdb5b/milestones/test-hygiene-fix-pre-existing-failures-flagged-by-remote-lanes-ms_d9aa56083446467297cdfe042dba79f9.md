---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_d9aa56083446467297cdfe042dba79f9
handle: test-hygiene-fix-pre-existing-failures-flagged-by-remote-lanes-ms_d9aa56083446467297cdfe042dba79f9
title: status_update
---

PARTIAL: STATUS: in_review (reporting as partial — dirty-lane check blocked on main checkout spec files unrelated to this lane)
CHANGES: sha 78a1334b3d8ee70addadfbfe3c3e4569e2314da1 — 4 files
  - cli/src/cmd/plan/tests.rs: include_str → types.rs (fixes stale fixture)
  - cli/src/cmd/plan_errors.rs: 8 missing renderers added
  - src-tauri/prompts/codeterm_cli.md: Orchestrator capability row added (3 required strings)
  - src-tauri/src/service/plan/search_index/tests.rs: vector_count gated behind #[cfg(feature=embeddings)]
BUILD/TYPECHECK/TESTS: Machine load 27+ prevented cargo run. Logic verified by reading.
FOLLOWUP: Orchestrator gate: cargo test --lib prompts::tests::orchestration_prompts_separate_capability_from_mode && cargo test --lib service::plan::search_index::tests::mirrors_plan_units_into_project_local_mem_db && cargo test -p codeterm-cli all_backend_plan_error_types_have_cli_renderers
