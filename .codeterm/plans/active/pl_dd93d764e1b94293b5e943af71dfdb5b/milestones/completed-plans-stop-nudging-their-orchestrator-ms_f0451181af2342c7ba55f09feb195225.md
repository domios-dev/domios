---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_f0451181af2342c7ba55f09feb195225
handle: completed-plans-stop-nudging-their-orchestrator-ms_f0451181af2342c7ba55f09feb195225
title: testing
---

REPRO DATA attached (Dali@eight, plan pl_3d12aae4163244c5acf6a1e8a8d0ecab): nudge = <codeterm-state-reconciliation> 'owns an execution-mode plan with unfinished work' while plan is 4/4 done, coverage 100%, drift 0, mode approved, orchestrator still assigned (pane 32787077), planner NULL, all tasks cleanup_status=None + unassigned. Dali's suspected conditions, all to be covered by the fix + tests: (a) reconciliation nudge fires on (execution-mode AND orchestrator-assigned) without gating on done==total; (b) the NEW planner-pairing mandate nudge (his own Lane 1) may also fire on completed plans — pairing pressure must gate on plans with unfinished/active work only; (c) cleanup_status=None on done tasks may read as cleanup debt — define None-on-never-laned as clean. Fix direction confirmed: single is_plan_complete predicate gating ALL orchestrator nudges (reconciliation + pairing + reminders); 100%-done approved plan = quiescent + one-time archive suggestion. Dali volunteers cross-machine verification on eight.
