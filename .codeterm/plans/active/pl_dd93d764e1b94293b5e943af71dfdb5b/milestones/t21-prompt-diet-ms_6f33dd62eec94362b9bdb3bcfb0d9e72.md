---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_6f33dd62eec94362b9bdb3bcfb0d9e72
handle: t21-prompt-diet-ms_6f33dd62eec94362b9bdb3bcfb0d9e72
title: gate
---

cli-agent eval evidence: baseline runs/20260703-081642-baseline (16 cases, claude/sonnet, old prompts); final runs/20260703-after3 (16 cases, new prompts, FIXED harness) — 15/16 CLI-first incl. all mem/mesh/docs cases; soft spots: identify uses --version not identify verb, agent.providers one variance wander. HARNESS BUGS FOUND+FIXED during the run: (1) dc send-text broken since the send rename — batches spawned 0 agents silently; (2) backgrounded case subshells drained the batch-file stdin; (3) CRITICAL VALIDITY BUG: sandboxes lived inside the repo so bench agents received the repo's checked-in AGENTS.md instead of the generated prompt — all prior runs of this benchmark tested the wrong context; sandboxes now mktemp under /tmp; (4) batch.sh accepts a model arg (runs pinned to sonnet per user).
