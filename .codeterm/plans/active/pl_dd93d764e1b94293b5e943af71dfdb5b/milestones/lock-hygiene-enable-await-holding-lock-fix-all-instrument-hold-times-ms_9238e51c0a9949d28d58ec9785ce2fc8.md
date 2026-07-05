---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_9238e51c0a9949d28d58ec9785ce2fc8
handle: lock-hygiene-enable-await-holding-lock-fix-all-instrument-hold-times-ms_9238e51c0a9949d28d58ec9785ce2fc8
title: status_update
---

in_review — await_holding_lock DENIED (allowlist removed from pre-push); lock_hold.rs TimedMutex/TimedRwLock instruments 5 hot locks with dev-only [LOCK-HOLD]>50ms, zero release overhead; launch.rs pty guard extracted before mux.write; zero same-body await-holding violations (3 static scans); fmt-check clean. Warm-gate followup: cargo clippy -D clippy::await_holding_lock + lock_hold unit tests + 10min soak grep.
