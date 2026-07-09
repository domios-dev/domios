---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_ee7d4077f3a341ffb1e26da52eb47b72
handle: test-hygiene-fix-pre-existing-failures-flagged-by-remote-lanes-ms_ee7d4077f3a341ffb1e26da52eb47b72
title: report_refused
---

PARTIAL: REPORT NOT SENT
readiness report refused: this git worktree has uncommitted lane changes.
Next commands:
  git status --short
  git add <files>
  git commit -m "<message>"
  codeterm agent report --status in_review --sha $(git rev-parse HEAD) "..."
Or report WIP with:
  codeterm agent report --status partial "..."
Dirty lane files:
  .codeterm/specs/sp_d2c70550432f4a829c89cf4a4cb7c3c5.json
  .codeterm/specs/sp_d2c70550432f4a829c89cf4a4cb7c3c5/INDEX.md
  .codeterm/specs/sp_d2c70550432f4a829c89cf4a4cb7c3c5/sections/board-req.md
  .codeterm/specs/sp_d2c70550432f4a829c89cf4a4cb7c3c5/sections/canvas-req.md
  .codeterm/specs/sp_d2c70550432f4a829c89cf4a4cb7c3c5/sections/context-req.md
  .codeterm/specs/sp_d2c70550432f4a829c89cf4a4cb7c3c5/sections/detail-req.md
  .codeterm/specs/sp_d2c70550432f4a829c89cf4a4cb7c3c5/sections/list-req.md
  cli/src/args/plan.rs

Original readiness report that was refused locally:
STATUS: in_review
CHANGES: 4 files, sha 78a1334b
  - cli/src/cmd/plan/tests.rs: include_str now reads types.rs (not mod.rs — types moved there, mod.rs has 0 type fields)
  - cli/src/cmd/plan_errors.rs: 8 missing renderers added (park_requires_reason, qa_note_required, qa_slot_unavailable, qa_not_running, gate_not_found, gate_already_answered, gate_requires_question, waiting_focus_requires_open_gate)
  - src-tauri/prompts/codeterm_cli.md: added Orchestrator row with 'You are not automatically an Orchestrator' + 'Make Orchestrator' + 'codeterm docs ORCHESTRATION'
  - src-tauri/src/service/plan/search_index/tests.rs: vector_count assert gated behind #[cfg(feature='embeddings')]

BUILD/TYPECHECK/TESTS: Could not run focused cargo tests — machine load hit 27+ (stopped per instruction). Diff reviewed manually: logic correct for all 3 fix areas. Orchestrator should run cargo test in warm main checkout to gate.

FOLLOWUP: Dali/eight cross-machine confirmation still needed per task test_plan. Run: cargo test --lib prompts::tests::orchestration_prompts_separate_capability_from_mode && cargo test --lib service::plan::search_index::tests::mirrors_plan_units_into_project_local_mem_db && cargo test -p codeterm-cli all_backend_plan_error_types_have_cli_renderers
