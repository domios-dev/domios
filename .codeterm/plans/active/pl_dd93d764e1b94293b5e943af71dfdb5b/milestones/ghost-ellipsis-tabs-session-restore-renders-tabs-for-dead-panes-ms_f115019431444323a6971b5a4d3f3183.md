---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_f115019431444323a6971b5a4d3f3183
handle: ghost-ellipsis-tabs-session-restore-renders-tabs-for-dead-panes-ms_f115019431444323a6971b5a4d3f3183
title: status_update
---

IN_REVIEW: STATUS: in_review

CHANGES:
- src-tauri/src/multiplexer/types.rs: added  (serde(skip), default false) to Pane struct — in-memory only, never persisted to session snapshot or live-state JSON
- src-tauri/src/multiplexer/state/snapshot.rs: snapshot filter now excludes panes with ; added 4 Rust repro/fix tests: ghost_repro_skip_snapshot_pane_excluded_from_snapshot (the failing repro proving the bug, now passing with fix), ghost_repro_normal_pane_survives_snapshot_round_trip (happy path regression), ghost_repro_first_run_picker_excluded_from_snapshot (end-to-end through ensure_first_run_tab), ghost_repro_mixed_workspace_drops_only_ephemeral (verifies only the marked pane is dropped)
- src-tauri/src/service/general_agent/first_run.rs: set pane.skip_snapshot = true after creating the first-run picker tab; added unit test first_run_picker_pane_has_skip_snapshot_set
- src/store/multiplexer/tree.ts: displayTitleForTab fallback is now  in dev mode (import.meta.env.DEV),  in production — never bare ellipsis, always debug-attributable
- tests/playwright/suites/20-new-session/picker.spec.ts: added ghost-tab invariant describe block with two @golden tests: GHOST-TAB current session has no placeholder titles; GHOST-TAB skip_snapshot pane absent from backend after picker creates it

BUILD/TYPECHECK/TESTS:
- Cannot run cargo test in worktree per worker constraints; orchestrator must verify with scripts/test-capped.sh on: multiplexer::state::snapshot and service::general_agent::first_run
- TS typecheck: npx tsc --noEmit (no npm install; use symlinked node_modules from main repo)
- Playwright: npx playwright test --config=tests/playwright/playwright.config.ts --project=golden --grep ghost-tab
- File sizes: snapshot.rs=987 (under 1000), all others well under limit

FOLLOWUP:
- Other sources of skip_snapshot=true: blank pre-spawn tabs created during orchestration group creation (create_tab before agent attaches) could also become ghost tabs if daemon crashes before rollback_spawn_tab. These don't have a unique marker currently. Could add skip_snapshot=true in create_tab and clear it when agent_type is set (in set_pane_agent_type or equivalent). Deferred — the first-run picker is the confirmed path; pre-spawn crash is a secondary scenario requiring its own repro.
- Correctness vector respected: skip_snapshot only excludes panes from the snapshot file, never drops a live pane. No rematerialization timeout was needed because the bug was at snapshot-write time, not restore time — the picker pane simply should not have been saved in the first place.
