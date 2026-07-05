---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_nb5slh
handle: spawn-guard-worker-into-the-canonical-checkout-requires-explicit-opt-in-ms_nb5slh
title: status_update
---

IN_REVIEW: STATUS: in_review (CODE-ONLY lane, gates deferred to QA queue)
CHANGES: Spawn guard for Worker-into-canonical-checkout. New src-tauri/src/service/spawn_guard.rs (predicate + ServiceContext glue + unit tests). Wired into both spawn entry points (async spawn_agent + sync spawn_agent_sync_with_ready). Added task_id + allow_shared_checkout to SpawnAgentParams/from_json/REST body/session_routes. CLI: agent spawn --allow-shared-checkout + --task-id (env CODETERM_TASK_ID), plumbed local + mesh. 13 files.
BEHAVIOR: Worker whose effective cwd == workspace primary cwd OR is a primary git checkout (git-dir==git-common-dir) is REFUSED with the exact 'git -C <cwd> worktree add ../<slug> -b lane/<slug>' fix + --allow-shared-checkout hint. --allow-shared-checkout proceeds AND records a visible 'exception' milestone stride on the bound task (add_kind=true). Linked worktrees pass silently. Orchestrator/General exempt; Planner profile exempt. Mesh spawns evaluate against the remote workspace cwd (guard runs on the remote daemon). Non-git dirs not canonical unless they equal the workspace cwd.
BUILD/TYPECHECK/TESTS: not run (CODE-ONLY). Standalone rustfmt --check clean on all 13 files (pre-push light gate passed).
GATES REQUESTED (QA queue):
 1. cargo check + clippy (src-tauri + cli)
 2. cargo test spawn_guard (5 predicate unit tests: role matrix, primary-vs-worktree, share-workspace-dir, non-git dir) -- capped
 3. Live spawn drills on dev for the 5 acceptance evals: (a) Worker cwd=canonical -> refused non-zero + fix text; (b) +--allow-shared-checkout -> spawns + exception stride on task; (c) Worker cwd=worktree -> allowed silent; (d) Orchestrator cwd=canonical -> allowed; (e) evals/orchestrator-creation-matrix stays green.
FOLLOWUP: task_310d7e6b (plan task dispatch) is post-release and will make isolated worktrees the default path; --read-only exemption is deferred to that (interim --allow-shared-checkout covers read-only lanes). Exception stride is best-effort: records on bound task when plan_id+task_id present, else logs a warning (solo-repo use case may have no plan binding).
