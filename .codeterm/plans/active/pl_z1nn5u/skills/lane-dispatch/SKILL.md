---
title: Manual lane dispatch (worktree + worker + bind)
area: orchestration
tags: [plan-skill]
check: codeterm plan task list pl_z1nn5u --status in_progress
---

# Manual lane dispatch (worktree + worker + bind)

Until `plan task dispatch` (t1-dispatch) lands, launching a worker lane is 4 manual steps.
`agent spawn --role Worker` REFUSES the canonical checkout, and `plan task assign` binds the
pane but does NOT flip status — you set `in_progress` + lane metadata yourself.

## When to use

Dispatching any worker lane on pl_z1nn5u before the one-command `plan task dispatch` exists.

## Steps

1. **Worktree** (never spawn a Worker into the canonical checkout):
   `git worktree add /Users/rollacode/Developer/ct-lanes/<slug> -b lane/<slug> feat/canvas-updates`
2. **Spawn** into it (capture `pane_id` from `--json`):
   `codeterm agent spawn claude --role Worker --parent <orch-pane> --plan <plan> --report-to <orch-pane> --cwd <worktree> --title "lane <slug>" --task-file <brief>`
3. **Assign** (exports CODETERM_TASK_ID to the pane):
   `codeterm plan task assign --pane <pane> <plan> <task_id>`
4. **Flip status + record lane** (assign does NOT do this):
   `codeterm plan task update <plan> <task_id> --status in_progress --pane <pane> --branch lane/<slug> --worktree <worktree>`

Brief = lean: role line + lane path (pre-created, don't switch branches) + code-only discipline
(narrow checks only, orchestrator owns QA gates) + AGENTS.md hard rules + exact
`codeterm agent report "...branch/SHA/worktree/tests..."`. Point the worker at
`codeterm task show <id>` for the full spec (body/criteria/test_plan/correctness all render).

## Gotchas

- `agent spawn` warns "agent cwd differs from workspace cwd" for worktree lanes — benign, expected.
- `plan task assign` alone leaves the task `pending`; without step 4 the Board shows no active work.
- Worktrees live in `../ct-lanes/<slug>` (outside the repo) so file-size scans stay clean.

## Check

Verify with: `codeterm plan task list pl_z1nn5u --status in_progress`
