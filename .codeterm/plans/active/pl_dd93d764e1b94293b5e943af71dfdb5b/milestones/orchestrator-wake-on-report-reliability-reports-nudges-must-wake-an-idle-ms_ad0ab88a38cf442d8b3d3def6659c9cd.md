---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_ad0ab88a38cf442d8b3d3def6659c9cd
handle: orchestrator-wake-on-report-reliability-reports-nudges-must-wake-an-idle-ms_ad0ab88a38cf442d8b3d3def6659c9cd
title: status_update
---

Closed after reconstruction audit. The accepted wake-on-report confirmer is already present in integration head ce525b51: Messenger::deliver calls confirm_wake_async; pty_io::wake implements per-PTY latest-body coalescing, bounded idle/waiting-only Enter nudges, no Working/Dead nudges, and second-delivery/no-double-submit regressions. Warm gate: TEST_CAP_SECS=180 scripts/test-capped.sh --manifest-path src-tauri/Cargo.toml --lib service::pty_io::wake => 7/7 pass, 2661 filtered out. v2 worker branch was a no-op verification branch.
