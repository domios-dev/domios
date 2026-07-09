---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_c773234efa8b451a9fad669324595845
handle: ghost-ellipsis-tabs-session-restore-renders-tabs-for-dead-panes-ms_c773234efa8b451a9fad669324595845
title: status_update
---

Merged and pushed ghost ellipsis tabs fix at 6482750c. Gates: npx tsx src/store/multiplexer/tree.test.ts; scripts/test-capped.sh --manifest-path src-tauri/Cargo.toml --lib ghost_repro (5/5); scripts/test-capped.sh --manifest-path src-tauri/Cargo.toml --lib purge_snapshot_ghosts (2/2); file-size/diff checks; push hook cargo check/fmt/clippy.
