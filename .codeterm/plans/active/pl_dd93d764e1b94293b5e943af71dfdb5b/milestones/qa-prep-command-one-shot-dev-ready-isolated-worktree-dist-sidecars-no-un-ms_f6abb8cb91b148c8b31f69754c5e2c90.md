---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_f6abb8cb91b148c8b31f69754c5e2c90
handle: qa-prep-command-one-shot-dev-ready-isolated-worktree-dist-sidecars-no-un-ms_f6abb8cb91b148c8b31f69754c5e2c90
title: testing
---

GATE GREEN + ACCEPTED — guard fix verified from the REAL scenario: ./build.sh qa-prep with inherited prod CODETERM_API_PORT=7685 and NO override -> rc=0 cold 3s / warm 2s; guard no longer blocks (first output line is the vite stage, not the refuse-prod error). dist/index.html OK, cli 11.7MB + embed 30MB reused from canonical. All AC met (one-command dev-prep, no tsc, sidecar reuse, staged+timeout, 2nd-run warm timing=2s). Accepting to done.
