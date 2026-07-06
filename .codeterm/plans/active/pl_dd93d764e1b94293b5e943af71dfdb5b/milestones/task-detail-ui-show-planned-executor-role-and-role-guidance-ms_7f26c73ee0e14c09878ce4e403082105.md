---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_7f26c73ee0e14c09878ce4e403082105
handle: task-detail-ui-show-planned-executor-role-and-role-guidance-ms_7f26c73ee0e14c09878ce4e403082105
title: decision
---

SCOPE ADDITION (round-96b, Andrey: 'планер заранее должен это планировать каждой задаче'): roles are planned UP FRONT for every task, enforced at three layers, same lane (one theme: roles are first-class task fields): (1) PLANNER CONTRACT — planner_suffix.md step 5 currently says 'role_guidance when the worker brief needs a specialized stance'; change to MANDATORY: every task carries executor_role (the label) AND role_guidance (the stance) at creation — 'when needed' becomes 'always; a task with no planned executor is not dispatchable'. Wiring test on the phrase. (2) ADD-TIME GUARD — extend task_quality_guard_hint (cli/src/cmd/hints.rs:36, already warns on missing criteria/test-plan/correctness) to also list a missing executor_role/role_guidance: the WARNING fires at creation, the exact moment the planner can fix it. (3) DOCTOR — verify the existing executor-role doctor warning escalates properly under --kind quality (it existed but stayed silent through 28 empty tasks; task_67d426a8 touched it — confirm severity, raise to the WARNING->ERROR ladder if plan is approved/execution). Display (original scope) + contract + guards together close the loop: planner must set it, CLI reminds at the moment, doctor catches drift, UI shows it.
