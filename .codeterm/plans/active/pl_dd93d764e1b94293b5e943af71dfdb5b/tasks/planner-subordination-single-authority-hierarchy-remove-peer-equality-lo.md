---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_831245e50c3e4683be1b026ab4b19814
handle: planner-subordination-single-authority-hierarchy-remove-peer-equality-lo
title: 'Planner subordination: single-authority hierarchy, remove peer-equality logic'
status: done
---

## What

FINAL hierarchy ruling (round-19, supersedes the remaining "peer equality" semantics from Wave 21): the Planner is SUBORDINATE to the Orchestrator — a specialized planning worker nested inside the orchestration group, not a co-equal authority. The user saw the live onboarding still saying "Peer contract... the Planner is the structural authority... structural changes require Planner acknowledgment" — all of that authority-inversion logic must be REMOVED, which simplifies the system.

## New model (single authority)

- The ORCHESTRATOR is the single authority over the plan: structure, tasks, dispatch, gates. No acknowledgment requirements, no deference rules, no "structural authority" split.
- The PLANNER is the orchestrator's planning specialist: it does GOAL/Spec/Plan/task-graph work WHEN THE ORCHESTRATOR DELEGATES it (which the orchestrator should strongly prefer over inline structure work — division of labor stays, authority inversion goes). It reports to the orchestrator like any worker; first child in the group; never spawns agents.
- planner_ref binding, the pairing mandate on promotion, doctor pairing rules, and the group UI (planner first child) all STAY — they are about wiring, not authority.
- Decision gates: unchanged mechanically, reframed — the orchestrator routes questions to the planner as its specialist (or escalates to the user); the planner does not "own" decisions.

## Rewrite list (grep-sweep for the old framing; wording is the worker's craft, framing is pinned)

1. src-tauri/src/prompts.rs PEER_* constants: PEER_ORCH_DEFERENCE_LINE deleted or rewritten to delegation ("Plan structure work goes to your Planner — delegate it, review the result; you remain the authority"); PEER_PLANNER_CHARTER_LINE -> "You are the plan's Planner, working under the Orchestrator: shape goal/spec/plan/tasks as directed, report back, never implement or spawn"; PEER_PAIRING_MANDATE_BLOCK keeps the mandatory spawn but drops "peers/equal" framing; greeting keeps warmth without equality claims.
2. The live reminder from the screenshot ("defer to the Planner peer for structural decisions... without Planner acknowledgment") — DELETE the acknowledgment machinery entirely (reminders.rs / planning-state lane's constants).
3. planner_suffix.md: planner works on delegation, reconciliation-of-foreign-mutations section simplifies (orchestrator edits are authoritative, planner reconciles FOR him, not against him).
4. docs/ORCHESTRATION.md peer/planning sections + docs/contributing/agents.md: single-authority framing.
5. Guide (src/help/content/en.ts agents section): rewrite "Neither is the boss of the other... equals" paragraph -> orchestrator-led with a dedicated planning specialist; UPDATE content.test.ts (currently asserts /peer|equals|same level/ — flip the assertion to the new framing).
6. Hierarchy figure (src/help/figures): planner moves under the orchestrator (first child), workers beside/below it.
7. Onboarding "## Peer contract" section (orchestration pair spawn path) -> "## Your Planner".

## Simplification dividend (the user's point)

Remove: acknowledgment gates, deference reminders, "foreign mutation" correction strides against the orchestrator, any dual-authority branching. Net-negative diff expected outside the constants.
