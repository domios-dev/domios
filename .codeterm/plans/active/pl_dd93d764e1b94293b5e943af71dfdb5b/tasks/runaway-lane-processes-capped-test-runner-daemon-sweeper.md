---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_87f7d2463a564453a5fd51ee94e4e2f3
handle: runaway-lane-processes-capped-test-runner-daemon-sweeper
title: 'Runaway lane processes: capped test runner + daemon sweeper'
status: done
---

## What

Systemic fix for runaway lane processes. Incident (round-16): a worker's `cargo test plan_skill` hung for ~2 HOURS at 240% CPU in its worktree — the worker waited instead of killing it, the machine sat at 40%+ system load until the user noticed in Activity Monitor. The discipline ("a test hanging past 90s is a deadlock in your code") exists only as prose; nothing enforces it.

## Three layers

### 1. Capped test runner (the tool workers must use)

`scripts/test-capped.sh <cargo-test-args...>` — portable bash (macOS has no `timeout(1)`): starts `cargo test` in its own process group, watchdog kills the WHOLE group after `${TEST_CAP_SECS:-90}` seconds, prints the output tail and a loud `TEST TIMED OUT AFTER ${cap}s — this is a deadlock/busy-loop in the code under test, not a slow build` banner, exits 124. Passes through exit codes otherwise. Works from worktrees (relative target dirs untouched).

### 2. Discipline made mechanical (prompts + docs)

- `testing_discipline.md` + worker charter constant gain one pinned line: "Run Rust tests ONLY through `scripts/test-capped.sh` — a bare `cargo test` in a lane is a violation; if the cap fires, read the code, do not re-run with a bigger cap."
- AGENTS.md testing section: same rule, two sentences, plus the incident rationale.

### 3. Daemon runaway sweeper (the orphan killer)

Periodic daemon task (5-min cadence, piggyback an existing background scheduler): find processes whose executable path matches `*/target/*/deps/*` (Rust TEST binaries specifically — never cargo/rustc/link, builds are legitimately long; never the daemon) with elapsed runtime > `runaway.test_binary_max_secs` (default 900). For each: SIGKILL, WARN log `[runaway] killed test binary <path> pid=<n> elapsed=<t>`, and if the binary's worktree path maps to a lane with a bound pane — nudge that pane: "your test binary <name> was killed after <t> min (runaway); the hang is in the code under test." Config-gated (`runaway.enabled`, default true), macOS+Linux first (sysinfo crate already in deps for process inspection — verify; else ps parsing), Windows follow-up noted.

## Executable test plan

1. `bash scripts/test-capped.sh --manifest-path cli/Cargo.toml plan_skill_hang_fixture` where the fixture test contains `loop {}` — PASS: exits 124 within cap+5s, banner printed, no `codeterm-*` deps binary left in `ps` (assert via `pgrep -f 'deps/codeterm' | wc -l` == 0).
2. Same invocation with a passing filter — PASS: exit 0, normal cargo output passthrough.
3. Sweeper unit test: mock process list with (a) deps binary at 20 min, (b) cargo at 30 min, (c) deps binary at 2 min — PASS: exactly (a) selected for kill.
4. Live dev check: start `cli/target/debug/deps/<any test bin> --nocapture` variant sleeping 20 min via the fixture, set runaway.test_binary_max_secs=60, observe WARN log + process gone within ~6 min — transcript in report.
5. Prompt wiring test references the charter constant (dedup rule).
