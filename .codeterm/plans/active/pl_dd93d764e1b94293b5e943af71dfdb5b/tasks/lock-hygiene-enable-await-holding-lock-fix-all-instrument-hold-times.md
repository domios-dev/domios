---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_d3ad1791c11d4b21a6cb8f1fc0bc33fe
handle: lock-hygiene-enable-await-holding-lock-fix-all-instrument-hold-times
title: 'Lock hygiene: enable await_holding_lock, fix all, instrument hold times'
status: done
---

## What

Make Rust lock hygiene mechanical instead of tribal. The UI stutters the user feels ("что-то в расте залочилось") comes from guards held across awaits/blocking work on paths the UI depends on — the reader race was one member of this class; the clippy allowlist currently SILENCES the exact lint that catches another (`-A clippy::await_holding_lock` in .githooks/pre-push).

## Three legs

1. **Enable the lint and fix everything it finds.** Remove `-A clippy::await_holding_lock` from the pre-push allowlist; run clippy; fix every violation PROPERLY (extract the needed data and drop the guard before the await, or restructure ownership — never wrap in a block as a silence trick). Each fix is reviewed against the lock-ordering doc (mux -> output_buf -> screen_state -> agent_tracker -> pty).
2. **Hold-time instrumentation (dev builds).** A feature-gated guard wrapper (or targeted instrumentation on the five ordered locks) that WARNs `[LOCK-HOLD] <lock> held <ms>ms` when a guard on a hot lock lives past ~50ms — same spirit as SLOW-IPC. Zero overhead in release (cfg-gated).
3. **Regression guard.** The lint stays denied in the pre-push full gate and CI; AGENTS.md lock-ordering section gains two lines documenting the lint + instrumentation.

## Executable test plan

1. `cargo clippy --manifest-path src-tauri/Cargo.toml --workspace --all-targets -- -D clippy::await_holding_lock` exits 0 (the proof the class is empty) — run via the integration gate, not a lane worktree.
2. Unit test for the instrumentation wrapper: guard held 60ms in test triggers the WARN callback; 10ms does not.
3. Manual dev soak: 10 minutes of normal use with agents spawning, grep dev log for LOCK-HOLD — findings become follow-up fixes or accepted with reasons in the report.
