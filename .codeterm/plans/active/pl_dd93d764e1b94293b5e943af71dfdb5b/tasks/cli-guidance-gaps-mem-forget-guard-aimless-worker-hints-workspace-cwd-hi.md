---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_e40a28ccd80e4124989a99e5e327e1c9
handle: cli-guidance-gaps-mem-forget-guard-aimless-worker-hints-workspace-cwd-hi
title: 'CLI guidance gaps: mem forget guard, aimless-worker hints, workspace cwd hint'
status: done
---

Planner audit of the CLI through the guidance lens (teach/remind/detect/wall) found three unowned gaps; user granted an explicit exception for the Planner to implement directly (notify orchestrator, QA gate respected — no local builds, gates via the machine QA runner). GAP 1 (wall-tier): 'codeterm mem forget <area>' batch-deletes an entire memory area with ZERO confirmation — add the same guard plan delete has (validate_plan_delete_confirmation precedent, cli/src/cmd/plan.rs:80): refuse without --yes, message names the area and item count when cheaply available. GAP 2 (teach-tier): 'agent spawn --role Worker' without --parent silently produces a worker whose reports go NOWHERE (default report target is the caller pane — from a human shell that is a dead end), and without --task an aimless worker; add warning hints (not refusals — refusal-tier cwd guard is task_46d7cc4c's, different concern): missing --parent -> 'worker reports will land in THIS pane; pass --parent <orchestrator pane> so reports reach the orchestrator'; missing --task -> 'worker spawned without a task brief; pass --task or the worker starts idle'. GAP 3 (teach-tier): 'workspace create' without --cwd -> agents spawned there inherit the daemon cwd (documented past incident class); hint: 'no --cwd: agents in this workspace inherit the daemon working directory; pass --cwd <project dir> for project work'. All three follow the existing hint/guard idioms (hints.rs push_hints/print_hints, validate_*_confirmation), respect the output contract (hints never displace the machine-readable last line — coordinate with task_dd7206eb's contract), and are product-general.
