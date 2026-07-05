---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_0zxrcn
handle: spawn-guard-worker-into-the-canonical-checkout-requires-explicit-opt-in-ms_0zxrcn
title: status_update
---

CODE-ONLY lane done + pushed. Branch lane/task-46d7cc4c-spawn-guard @ f94b2e2b. Spawn guard implemented: Worker with effective cwd == workspace primary cwd OR primary git checkout (git-dir==git-common-dir) is refused with exact worktree-add fix text; --allow-shared-checkout proceeds and records a visible exception stride on the bound task. Worktrees pass silently; Orchestrator/General/Planner exempt; mesh evaluates remote workspace cwd. New file src-tauri/src/service/spawn_guard.rs (predicate+glue+5 unit tests). CLI: agent spawn --allow-shared-checkout + --task-id. 13 files. rustfmt --check clean; light pre-push gate passed. GATES REQUESTED for QA: cargo check+clippy (src-tauri+cli), cargo test spawn_guard (capped), 5 live acceptance spawn drills on dev + orchestrator-creation-matrix.
