---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_0eebf699302e47e1b5a124bae7dcd6ca
handle: runaway-lane-processes-capped-test-runner-daemon-sweeper-ms_0eebf699302e47e1b5a124bae7dcd6ca
title: decision
---

Scope upgrade for layer 1 (round-16b): prefer cargo-nextest as the primary test runner — it gives per-test timeouts natively (slow-timeout with terminate-after), process-per-test isolation, and faster parallel runs; scripts/test-capped.sh becomes the fallback for environments without nextest and for non-test cargo invocations. Add: install check + .config/nextest.toml with slow-timeout = { period = 30s, terminate-after = 3 } profile, charter line updated to 'run tests via cargo nextest run <filter>'. The daemon sweeper (layer 3) stays as the backstop.
