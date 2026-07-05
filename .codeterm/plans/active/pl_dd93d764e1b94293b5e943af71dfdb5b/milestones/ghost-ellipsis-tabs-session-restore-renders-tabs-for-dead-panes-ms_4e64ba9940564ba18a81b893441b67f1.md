---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_4e64ba9940564ba18a81b893441b67f1
handle: ghost-ellipsis-tabs-session-restore-renders-tabs-for-dead-panes-ms_4e64ba9940564ba18a81b893441b67f1
title: status_update
---

IN_REVIEW: Two-layer fix for ghost ellipsis tabs on session restore.

PRIMARY REPRO (ghost_repro_restore_from_old_snapshot_drops_picker_pane):
- Builds a pre-fix session snapshot with picker pane marker present
- Proves bug: picker pane IS in restored state after from_session_snapshot
- Applies fix: purge_snapshot_ghosts drops tab, records to Recently Closed
- Verifies: pane absent, tab count 0, recently_closed non-empty

PREVENTION (skip_snapshot):
- Pane.skip_snapshot field (serde-skipped, defaults false)
- ensure_first_run_tab sets skip_snapshot=true on picker pane
- to_session_snapshot() filters out panes where hidden || skip_snapshot

DETECTION (purge_snapshot_ghosts):
- Called at startup in states.rs after attach_closed_store
- Scans all restored workspaces for panes with FIRST_RUN_PANE_MARKER description
- Drops via close_tab (→ Recently Closed + INFO log)
- Handles old snapshots saved before skip_snapshot was enforced

REGRESSION (happy path covered):
- ghost_repro_normal_pane_survives_snapshot_round_trip
- ghost_repro_mixed_workspace_drops_only_ephemeral
- purge_snapshot_ghosts_preserves_real_panes

FRONTEND (title fallback):
- displayTitleForTab: bare ellipsis replaced with [pane:hex] (dev) or hex (prod)
- Picker tabs use virtualTabs (frontend-only), never reach backend snapshot

FILE SIZE: snapshot.rs split into snapshot/mod.rs (381L) + snapshot/tests.rs (700L)
