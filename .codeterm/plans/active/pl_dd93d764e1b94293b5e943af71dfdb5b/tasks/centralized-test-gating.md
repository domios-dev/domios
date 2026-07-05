---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_fac7df2cff5c42f5b907d67592e4cea4
handle: centralized-test-gating
title: Centralized test gating for worker lanes
status: done
---

Problem: workers in separate worktrees are running broad build/test gates (cargo check, pnpm test:unit, vite build) and each worktree warms its own target/node caches, causing CPU spikes and long waits on this machine. We need an orchestration/testing process where workers run only narrow focused tests while the Orchestrator or a dedicated test-runner lane runs heavier gates from a central warm checkout/cache.

Design and implement the process/docs/prompt updates:
- Worker briefs must say: no broad cargo/pnpm/vite/playwright gates unless explicitly assigned; run focused tests only and report exact commands.
- Orchestrator owns broad gates after integration in the main/warm checkout, or spawns a single test-runner worker for serialized verification.
- Document how to reuse warm caches safely, avoid per-worktree cold target rebuilds, and run one cargo test at a time.
- Add this to docs/ORCHESTRATION.md and any worker/orchestrator prompt snippets that generate worker briefs.
- If there is a standard worker preamble, update it.
