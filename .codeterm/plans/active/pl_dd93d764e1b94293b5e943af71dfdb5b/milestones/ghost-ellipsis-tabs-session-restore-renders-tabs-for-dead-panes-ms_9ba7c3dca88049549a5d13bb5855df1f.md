---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_9ba7c3dca88049549a5d13bb5855df1f
handle: ghost-ellipsis-tabs-session-restore-renders-tabs-for-dead-panes-ms_9ba7c3dca88049549a5d13bb5855df1f
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
  .codeterm/specs/sp_d2c70550432f4a829c89cf4a4cb7c3c5/sections/active-work.md

Original readiness report that was refused locally:
TS gate fixed; Rust gate deferred to integration checkout per orchestrator instruction.

TS FIX (gate 1):
- displayTitleForTab: import.meta.env.DEV → import.meta.env?.DEV
- Root cause: tsx test runner has import.meta but no import.meta.env; optional chaining makes it undefined (falsy) rather than throwing
- tree.test.ts output: ✓ displayTitleForTab uses pane.title when present / ✓ falls back to pane_id prefix when pane title is empty / ✓ falls back to tab.pane_id prefix when pane is missing / ✓ tree tests green

RUST GATE (gate 2 — commands to run from warm integration checkout):
cd src-tauri
TEST_CAP_SECS=90 ../scripts/test-capped.sh -- --lib ghost_repro
TEST_CAP_SECS=90 ../scripts/test-capped.sh -- --lib purge_snapshot_ghosts
Tests that should pass: ghost_repro_restore_from_old_snapshot_drops_picker_pane, ghost_repro_skip_snapshot_pane_excluded_from_snapshot, ghost_repro_normal_pane_survives_snapshot_round_trip, ghost_repro_first_run_picker_excluded_from_snapshot, ghost_repro_mixed_workspace_drops_only_ephemeral, purge_snapshot_ghosts_drops_picker_pane_and_records_to_closed, purge_snapshot_ghosts_preserves_real_panes

Reason for deferral: worktree was cold (rav1e, ort-sys, tokenizers C++ deps — first build takes >5min); orchestrator stopped cargo invocations at load 50+. Integration checkout is warm.
