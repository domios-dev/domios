---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_9e992813a7604aff8f61a004e06cd6fc
handle: cli-test-binary-never-exits-args-parse-tests-hang-after-passing-macos
title: 'CLI test binary never exits: args-parse tests hang after passing (macOS)'
status: pending
---

FOUND AND PROVEN PRE-EXISTING by the refresh-cycle gate (round-100, Rossini): cli args-parse tests (e.g. args::orchestrator::tests::status_parses) HANG under cargo test — the assertion PASSES, then the test process never exits. Reproduced on release/v1.6.2 with a clean env, so it predates all current lanes; possibly macOS-only. CONSEQUENCE: mac CI/QA cannot verify any codeterm-cli parse tests — every run eats the 90s cap and the cap-kill has been silently misattributed to 'slow compile' (this is also the likely root of the OLD task_b68b897f manifest-freshness hang — same binary, same never-exits shape; if that task closed as non-repro or fixed-by-luck, this is its real cause resurfacing). HUNT: something in the CLI crate spawns a NON-DAEMON background resource at first touch — suspects in likelihood order: a once_cell/lazy_static holding a tokio runtime or reqwest blocking client (its connection-pool thread keeps the process alive), the socket-discovery/mesh probe init, or a logging/telemetry thread; a pure clap parse touching the args module should not construct ANY of those — so either an eager static in the crate root or a module-level init leaks into the test binary. FIX DIRECTION: make the offending init lazy (never constructed unless actually used) AND ensure background threads are detached/daemon-style or explicitly dropped; do NOT fix with process::exit in the test harness (masks the leak). ACCEPTANCE: 'cd cli && ../scripts/test-capped.sh args::' completes and EXITS in seconds on macOS (before/after timing recorded); the full cli test suite exits naturally on mac; identify and name the exact lingering resource in the report (which static, which thread).
