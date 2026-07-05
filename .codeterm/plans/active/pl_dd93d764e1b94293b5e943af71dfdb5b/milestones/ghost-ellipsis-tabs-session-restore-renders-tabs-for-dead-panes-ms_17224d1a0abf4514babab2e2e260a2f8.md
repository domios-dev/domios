---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_17224d1a0abf4514babab2e2e260a2f8
handle: ghost-ellipsis-tabs-session-restore-renders-tabs-for-dead-panes-ms_17224d1a0abf4514babab2e2e260a2f8
title: status_update
---

PARTIAL: TS gate fixed; Rust gate deferred to integration checkout per orchestrator instruction.

TS FIX (gate 1):
- displayTitleForTab: import.meta.env.DEV → import.meta.env?.DEV
- Root cause: tsx test runner has import.meta but no import.meta.env; optional chaining makes it undefined (falsy) rather than throwing
- tree.test.ts output: ✓ uses pane.title when present / ✓ falls back to pane_id prefix when pane title is empty / ✓ falls back to tab.pane_id prefix when pane is missing / ✓ tree tests green
- Commit: 8dba30da

RUST GATE (gate 2 — commands for warm integration checkout):
cd src-tauri
TEST_CAP_SECS=90 ../scripts/test-capped.sh -- --lib ghost_repro
TEST_CAP_SECS=90 ../scripts/test-capped.sh -- --lib purge_snapshot_ghosts

Expected passing tests: ghost_repro_restore_from_old_snapshot_drops_picker_pane, ghost_repro_skip_snapshot_pane_excluded_from_snapshot, ghost_repro_normal_pane_survives_snapshot_round_trip, ghost_repro_first_run_picker_excluded_from_snapshot, ghost_repro_mixed_workspace_drops_only_ephemeral, purge_snapshot_ghosts_drops_picker_pane_and_records_to_closed, purge_snapshot_ghosts_preserves_real_panes

Deferral reason: worktree cold (rav1e/ort-sys/tokenizers C++ deps = >5min first build); orchestrator stopped cargo at load 50+.
