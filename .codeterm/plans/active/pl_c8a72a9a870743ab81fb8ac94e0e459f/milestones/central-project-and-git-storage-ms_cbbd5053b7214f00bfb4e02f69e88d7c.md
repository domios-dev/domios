---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_cbbd5053b7214f00bfb4e02f69e88d7c
handle: central-project-and-git-storage-ms_cbbd5053b7214f00bfb4e02f69e88d7c
title: decision
---

ANDREY: must be ZERO-CONFIG/automatic — agents NEVER specify the main worktree. Requirement: project_root auto-derived from git-common-dir on every plan op (from the agent's cwd, any worktree → central root); project_path pinned on the plan at creation. After this, a worker in any worktree (even its own isolated dev) auto-resolves to the central plan store — no hardcoded port. Single-writer (funnel writes through one owning daemon + advisory lock) is the only remaining guardrail (review finding #2).
