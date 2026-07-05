---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_78b1543fe7ab4e56b6f5f4fd7aedefdf
handle: orchestrator-wake-on-report-reliability-reports-nudges-must-wake-an-idle-ms_78b1543fe7ab4e56b6f5f4fd7aedefdf
title: testing
---

Cross-machine Windows gate result from Hubble@ROG: PASS for wake-on-report reliability at exact sha bd0a041e240bdafdf876a5565f84a2d1476e00b6 on Windows 11 build 26200 / MSVC 14.44.35207. Isolated detached worktree, local checkout untouched. Command: TEST_CAP_SECS=120 scripts/test-capped.sh --manifest-path src-tauri/Cargo.toml --lib service::pty_io::wake, invoked via PowerShell-foreground per AGENTS.md Windows cargo discipline. Result: 7/7 pass, 0 failed, 2629 filtered out, no hang. Fresh worktree needed CLI/embed stubs and dist copy for tauri-build resource validation; unrelated to pty_io::wake logic.
