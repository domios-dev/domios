---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_e6c19f20224b4236af0cdbba354a867a
handle: cli-codeterm-plan-task-dispatch-one-command-worker-launch-from-a-task-ms_e6c19f20224b4236af0cdbba354a867a
title: decision
---

CLI FRICTION WAVE (round-57, Planner design at Orchestrator request). Findings from live session evidence: (1) dispatch = 4 manual commands with real mistake classes (--parent misses, id format mismatch, readback false-negative) -> designed 'plan task dispatch' (THIS task): composition+atomicity over existing primitives, routing defaults from the task's recorded model/why, daemon-side single RPC with rollback, --dry-run/--read-only/--gate/--force modes; (2) readback trap CONFIRMED AGAIN while filing this very wave (hint line after id ate the id in tail -1) -> output-contract task: last stdout line = canonical id + short-id resolver sweep (P1, de-risks remaining release dispatches); (3) plan_index_mirror false-fail is a REGRESSION of done task_354c755f (two recurrences cited: creation-matrix under load, Phase C mirror lag) -> verify-first fix task_7089039c (P1); (4) lane-hygiene visibility -> 'plan lanes' audit command task_d1450940 (P2, read-only, shares liveness logic with reconcile); (5) shell-pane completion contract: NOT duplicated here — TEXT D (prompt pattern, in flight via task_dc526c35/task_52539b4e) covers it now; native '--gate' sugar rides the dispatch command. PROPOSED SEQUENCE: P1 pair (output-contract, mirror-regression) dispatch NOW — they de-risk every remaining endgame lane; dispatch command + plan lanes are P2 POST-RELEASE (feature work must not delay the release gates; pull earlier only if dispatch friction actively blocks the burn-down).
