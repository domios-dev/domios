---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_c877126f86e443e9bbe56b5f5648b971
handle: task-activity-reaches-its-executors-strides-in-task-show-prompts-and-dis
title: 'Task activity reaches its executors: strides in task show, prompts, and dispatch briefs'
status: pending
---

GAP VERIFIED (round-97, Andrey's question exposed it): a task's ACTIVITY (strides — pinned decisions, scope additions, bounce findings, planner design notes) is INVISIBLE to the agents who pick the task up. Proof this is not theoretical: the mission-control v2 bounce (round-46, finding F1 'IGNORED RECORDED STRIDES') happened exactly because the worker never saw the two pre-recorded implementation notes — a full bounce round paid for this gap already. Today nothing tells a worker/QA/reviewer that strides exist: 'codeterm task show' does not render them, no prompt mentions reading them, and the dispatch design composes the worker brief without them. THREE-PART FIX: (1) CLI: 'codeterm task show --activity' (or activity section in default show, BOUNDED — last N strides, decision-kind first, respecting the output contract and the no-linear-growth rule from the siblings fix) so any agent can read a task's history in one command; (2) PROMPTS: one line in worker_suffix ('Before starting, read your task's activity: codeterm task show <task> --activity — decisions and bounce findings recorded there are binding parts of your brief') + the same expectation named in the QA/review charters; wiring tests; (3) DISPATCH: task_310d7e6b's prompt composition adds a 'Recorded decisions' section (decision-kind strides, most recent first, bounded ~10) between the body and the criteria — coordinate with that lane, do not duplicate its composer. Also verify the review-gate charter (task_5d154576) instructs reviewers to read strides for prior bounce context.
