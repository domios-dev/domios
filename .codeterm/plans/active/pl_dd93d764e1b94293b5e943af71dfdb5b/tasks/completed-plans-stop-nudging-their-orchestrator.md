---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_612a2e031f8b49f4a946b1a8292d8b16
handle: completed-plans-stop-nudging-their-orchestrator
title: Completed plans stop nudging their orchestrator
status: done
---

Field bug (Dali@eight, round-11): an orchestrator received a plan nudge although his plan appears fully finished. Repro data requested from Dali (nudge text + plan state) — attach to this task when it arrives. Investigate the nudge derivation (service/hierarchy plan_reminder_nudge + reminders): the nudge condition is 'owns execution-mode plan with unfinished work' — enumerate what counts as unfinished: non-terminal tasks (incl. in_review? blocked? parked?), open loops, cleanup debt (lane metadata cleanup-status), open decision gates, pending reports. Likely culprits: (a) cleanup debt or lane metadata left non-clean on done tasks keeps the plan 'unfinished'; (b) in_review/blocked tasks with no live pane; (c) plan mode never moved to complete. FIX: define the completed-predicate explicitly (single function, unit-tested): all tasks terminal AND no open loops/gates AND no cleanup debt => ZERO nudges; each nudge names WHY the plan is not complete (the blocking category) so silent phantom nudges are impossible; optionally auto-suggest 'plan mode complete / archive' when the predicate holds.
