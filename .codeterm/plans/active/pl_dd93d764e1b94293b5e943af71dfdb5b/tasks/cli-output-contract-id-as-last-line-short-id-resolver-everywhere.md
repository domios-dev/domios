---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_dd7206eb376d4c96bdab0726640555fc
handle: cli-output-contract-id-as-last-line-short-id-resolver-everywhere
title: 'CLI output contract: id as last line, short-id resolver everywhere'
status: in_progress
---

Two cheap fixes that caused real mistakes this session (Orchestrator AND Planner both hit them). (A) READBACK TRAP: mutating plan commands (task add, task update, record) sometimes print a trailing HINT line ('safe next step: read it back with...' / 'orchestrating? read codeterm docs...') AFTER the created id — so scripts doing tail -1 capture the hint instead of the id (this exact failure occurred repeatedly this session: created-task ids lost in automation). FIX as an output CONTRACT: for every mutating plan/task command, the LAST stdout line is the canonical result id (or --json object); hints move to stderr or above the id line; add a CLI test asserting last-line-is-id for task add/update/record. (B) SHORT-ID INCONSISTENCY: the unified resolver exists (shipped by the done 'Short ids + parent-free refs + unified resolver' task) but is not applied uniformly — some subcommands still require full ids (Orchestrator hit short/full mismatches during Phase C; audit which: task update, task show, record --task, plan get at minimum). Sweep EVERY plan/task id argument in cli/src through the resolver; add a matrix test (short id, full id, ambiguous prefix -> clear error listing candidates).
