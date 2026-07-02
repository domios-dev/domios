---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_82bd7e7793f94668b30923e3383eac73
handle: workspace-moved-tabs-visible-ms_82bd7e7793f94668b30923e3383eac73
title: status_update
---

STATUS: partial
CHANGES: src/components/Sidebar/workspaceTabs.ts (new), workspaceTabs.test.ts (new), Sidebar.tsx; src-tauri/src/service/spawn.rs (resolve_spawn_workspace_id + priority fix), orchestrator.rs, spawn_chatbackend.rs, spawn_tests.rs; src-tauri/src/socket/server/workspace.rs (move_tab via svc)
BUILD/TYPECHECK/TESTS: cargo test interrupted (lock/hang); workspaceTabs.test.ts fixed react import, re-run pending; lints clean on Sidebar files
FOLLOWUP: alive; current state: working — root cause identified: sidebar hid tabs whose parent_pane_id points to a pane in another workspace (moved/spawn-misplaced worker tabs); spawn paths preferred parent workspace over explicit --workspace (fixed in 4 Rust sites). Need Orchestrator gates: cargo test resolve_spawn_workspace_prefers_explicit, npx tsx workspaceTabs.test.ts, manual verify move-tab + spawn --workspace --parent in UI.
