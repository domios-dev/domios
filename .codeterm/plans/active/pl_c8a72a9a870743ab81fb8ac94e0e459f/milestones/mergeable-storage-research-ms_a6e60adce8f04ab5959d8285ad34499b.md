---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_a6e60adce8f04ab5959d8285ad34499b
handle: mergeable-storage-research-ms_a6e60adce8f04ab5959d8285ad34499b
title: decision
---

REJECT Dolt (git-for-data): no Go runtime in Rust/Tauri, data lives in a SEPARATE dolt repo with own remotes -> does NOT travel inside the project's git branches/worktrees (the hard requirement), GitHub PRs show no plan diffs, replaces derived sqlite (contradicts §21). Escape hatch only if SQL-grade cell merge at scale is ever needed — plan/stride volume never needs it.
