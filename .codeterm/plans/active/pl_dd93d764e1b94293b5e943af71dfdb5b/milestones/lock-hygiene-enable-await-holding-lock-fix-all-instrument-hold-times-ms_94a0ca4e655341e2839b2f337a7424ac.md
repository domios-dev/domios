---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_94a0ca4e655341e2839b2f337a7424ac
handle: lock-hygiene-enable-await-holding-lock-fix-all-instrument-hold-times-ms_94a0ca4e655341e2839b2f337a7424ac
title: status_update
---

IN_REVIEW: STATUS: in_review
BRANCH: lane/lock-hygiene (sha 713ff82)

CHANGES (10 files, +207/-25):
- .githooks/pre-push: removed '-A clippy::await_holding_lock' from CLIPPY_ALLOW (full gate now DENIES it).
- src-tauri/src/lock_hold.rs (NEW): TimedMutex/TimedRwLock wrappers. Debug build wraps guards, logs '[LOCK-HOLD] <name>.<op>() held Nms (threshold 50ms)' on drop when held >= LOCK_HOLD_THRESHOLD (50ms). Release build = #[repr(transparent)] parking_lot wrapper with #[inline(always)], zero release overhead. Guards Deref/DerefMut to inner T so all ~250 call sites are unchanged.
- Rewired the FIVE ordered hot locks to the wrapper via type aliases: mux (TimedRwLock) in multiplexer/commands.rs; pty/output_buf/screen_state/agent_tracker (TimedMutex) in lib.rs. Named at construction (startup/states.rs, session_persist.rs, inject_verify.rs, service/tests.rs).
- service/orchestrator/launch.rs: extract 'let spawn_result = svc.pty.lock().spawn(..)' before match, so the pty guard drops before the match body takes svc.mux.write() (was reverse of documented order pty(5)->mux(1)).
- AGENTS.md: lint+instrumentation rule under Lock ordering section.

AWAIT_HOLDING_LOCK AUDIT (findings + fix shape):
- Ran 3 independent static scans (named-guard-scope-tracked, loose-proximity, if-let/match/while-let scrutinee-temp with detach awareness). ZERO same-body guard-across-await violations: this lane already scopes guards into inner { } blocks or extracts .field.clone() before awaits.
- The only scrutinee-temp hit (launch.rs match svc.pty.lock().spawn(..)) has its .await inside a detached thread::spawn+block_on (separate MIR body) — clippy would NOT flag it, but it IS a lock-ORDER smell (pty held while mux.write taken). Fixed by extraction. Fix shape used everywhere: bind the extracted value, drop the guard at the ';', then match/await.

TRADEOFF (please note): clippy::await_holding_lock matches known std/parking_lot guard PATHS. The 5 wrapped hot locks now yield TimedGuard, which clippy does not recognize — so static await-holding detection on THOSE 5 is delegated to the runtime [LOCK-HOLD] log (an await-held hot guard shows as a multi-ms/second hold). All OTHER locks (config/mesh/update/etc, unwrapped) remain under static clippy deny. No hidden violations exist today (audit above).

BUILD/TYPECHECK/TESTS: NOT run full clippy/build in-lane per your high-load reminder + task boundary. Verified via 'cargo fmt --all --check' (EXIT 0, parse-clean whole crate) and 'rustfmt --check src/lock_hold.rs' (clean). Instrumentation unit tests written in lock_hold.rs (threshold_boundary: 0/49ms false, 50ms/500ms true; guard_derefs_to_inner) but NOT executed (cold build).

FOLLOWUP for orchestrator (warm checkout):
- Deny-lint gate: cargo clippy --manifest-path src-tauri/Cargo.toml --workspace --all-targets -- -D clippy::await_holding_lock
- Run instrumentation tests: scripts/test-capped.sh lock_hold (debug profile).
- 10-min dev soak + grep '[LOCK-HOLD]' ~/.codeterm-dev/logs/codeterm.log; triage any hits (expected: near-zero on the hot path).
- Optional hardening: a clippy.toml 'await-holding-invalid-types' entry for the wrapper guard could restore static detection on the 5 wrapped locks (skipped here to avoid a fragile private-path config that could break the gate).
