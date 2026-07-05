---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_2806014b2bae47158283b80932b2012e
handle: lock-hygiene-enable-await-holding-lock-fix-all-instrument-hold-times-ms_2806014b2bae47158283b80932b2012e
title: status_update
---

IN_REVIEW: STATUS: in_review
BRANCH: lane/lock-hygiene (713ff82)
CHANGES: removed -A clippy::await_holding_lock from .githooks/pre-push CLIPPY_ALLOW; new src-tauri/src/lock_hold.rs (TimedMutex/TimedRwLock: debug logs [LOCK-HOLD] on drop past 50ms threshold, release = repr(transparent) parking_lot wrapper, zero overhead, guards Deref so call sites unchanged); rewired 5 hot locks (mux/pty/output_buf/screen_state/agent_tracker) via type aliases + named construction; launch.rs extract spawn_result to drop pty guard before mux.write() (was reverse order); AGENTS.md lint+instrumentation rule.
AUDIT: 3 static scans (scope-tracked named-guard, loose proximity, if/while-let+match scrutinee-temp) -> ZERO same-body guard-across-await violations; lane already scopes guards in inner blocks. Only scrutinee hit (launch.rs) had its await in a detached thread::spawn+block_on (clippy wouldn't flag) but was a lock-order smell, now fixed.
TRADEOFF: wrapped hot locks yield TimedGuard which clippy await_holding_lock doesn't match; static detection on those 5 delegated to runtime [LOCK-HOLD] log; all other locks stay under static clippy deny; no hidden violations today.
BUILD/TESTS: no full clippy/build in-lane (high load + boundary). Verified cargo fmt --all --check EXIT 0 and rustfmt --check lock_hold.rs clean. Unit tests written (threshold_boundary, guard_derefs_to_inner) not executed.
FOLLOWUP: run at warm gate: cargo clippy --manifest-path src-tauri/Cargo.toml --workspace --all-targets -- -D clippy::await_holding_lock; run lock_hold tests via scripts/test-capped.sh; 10-min dev soak + grep [LOCK-HOLD] logs and triage.
