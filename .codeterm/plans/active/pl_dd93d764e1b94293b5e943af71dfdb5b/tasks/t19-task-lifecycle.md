---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_081738b89974451fbcc5f934418f1ba2
handle: t19-task-lifecycle
title: 'Task lifecycle: PARKED + relentless heartbeat escalation'
status: done
---

Task lifecycle + relentless heartbeat (round 4). Read spec section task-lifecycle first. Live motivation: t7-review sat pending 12h on this very plan with no lane, no reason, no escalation.

Steps:
1. Status model: add PARKED; transition requires --reason (recorded as working-point stride); unpark -> pending (stride). CLI: plan task park <task-ref> --reason / plan task unpark. Parked excluded from heartbeat-open set, folded in views (UI degradation acceptable until Wave 14 catches up: reuse done-folding).
2. Heartbeat: nudges continue while ANY task is pending/in_progress/in_review/blocked; nudge text states the contract verbatim ("repeats until every open task is dispatched, parked with a reason, or sent to replanning; silence is not an option") and lists the five exits.
3. Escalation: age thresholds (pending unstaffed >4h, in_review unjudged >2h — constants, documented) -> nudge names stale tasks; continued ignoring shortens cadence and records drift (extend stale_in_review drift to all open statuses).
4. Doctor: error parked-without-reason; warning aged unstaffed open tasks.
5. Tests: transitions, nudge text/escalation, drift, doctor; live scratch smoke: age a task past threshold, observe escalated nudge naming it; park-without-reason rejected.
6. ORCHESTRATION doc: lifecycle + exits + thresholds; revision bump.
