---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_4578b21c811942e79c6458632ea56fab
handle: cli-guidance-gaps-mem-forget-guard-aimless-worker-hints-workspace-cwd-hi-ms_4578b21c811942e79c6458632ea56fab
title: status_update
---

IN_REVIEW: STATUS: in_review (LANE REBUILT per your inspection)
BRANCH: lane/e40a28cc-cli-guidance @ 4c6e8964 — now based on origin/feat/canvas-updates @ 2cbe6567 (force-pushed with lease)
WORKTREE: /Users/rollacode/Developer/codeterm-lanes/e40a28cc-cli-guidance
CHANGES: identical scope, clean base — diff vs current head is EXACTLY the 7 CLI files, 95 insertions: mem forget --yes guard (local + mesh paths), worker spawn hints (builder in hints.rs, call in agent.rs), workspace --cwd hint (local_io.rs), unit tests for all three. The stale-base fmt repairs resolved themselves in rebase: lane_liveness dropped as already-upstream, task_ops part emptied out, only my own hint-fn wrap survived. No reverts of plugin-channel/dead-prompt/Codex-fixture/webFork — verified by the diffstat above.
TESTS RUN: none locally (code-only lane)
GATES REQUESTED: cargo check + clippy both crates; cargo test -p codeterm-cli (worker_spawn_hints_fire_only_for_workers_missing_bindings, mem_forget_refused_without_yes, mem_forget_allowed_with_yes, workspace_cwd_hint_names_the_flag); CLI manifest freshness (mem forget gained --yes/--confirm — docs/CLI.md may need regeneration via codeterm cli-manifest --write)
