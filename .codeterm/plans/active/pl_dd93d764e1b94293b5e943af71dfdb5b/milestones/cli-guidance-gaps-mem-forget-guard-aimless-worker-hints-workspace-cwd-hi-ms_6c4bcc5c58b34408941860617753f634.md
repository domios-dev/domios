---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_6c4bcc5c58b34408941860617753f634
handle: cli-guidance-gaps-mem-forget-guard-aimless-worker-hints-workspace-cwd-hi-ms_6c4bcc5c58b34408941860617753f634
title: status_update
---

IN_REVIEW: STATUS: in_review
BRANCH: lane/e40a28cc-cli-guidance @ 9e0cc5fb (base 7f651657)
WORKTREE: /Users/rollacode/Developer/codeterm-lanes/e40a28cc-cli-guidance
CHANGES: 3 CLI guidance gaps closed per task body — (1) mem forget now refuses without --yes on BOTH paths (local cli/src/cmd/mem/item.rs validate_mem_forget_confirmation + mesh cli/src/mesh_cli/api_map/actions.rs), args gain --yes/--confirm; (2) worker spawn without --parent/--report-to or --task prints teach-tier hints (builder in cli/src/cmd/hints.rs worker_spawn_hints — relocated there to keep agent.rs under the 1000-line limit — call site in cmd/agent.rs spawn arm, stderr only); (3) workspace create without --cwd prints the daemon-cwd hint (cli/src/cmd/local_io.rs). Unit tests added: forget refusal/allow, spawn-hint matrix (5 cases), cwd-hint content. Plus two fmt-repair commits for PRE-EXISTING drift on the integration head (lane_liveness/tests.rs, task_ops.rs fill_lane_tests) — fmt-only, verified by diff inspection.
TESTS RUN: none locally (code-only lane per one-QA-per-machine)
GATES REQUESTED: cargo check + clippy both crates; cargo test -p codeterm-cli (new: worker_spawn_hints_fire_only_for_workers_missing_bindings, mem_forget_refused_without_yes, mem_forget_allowed_with_yes, workspace_cwd_hint_names_the_flag); CLI manifest freshness check (args changed: mem forget gained --yes — regenerate docs/CLI.md if the manifest gate requires)
FOLLOWUP: integration head carried fmt drift that blocks any lane push (light pre-push runs fmt --check) — my lane includes the repair, but your merge gate should run fmt before accepting lanes or every parallel lane will hit it
