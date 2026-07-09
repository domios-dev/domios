---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_22358dd14a8d40c9b3a9f60c6e0e5fc2
handle: planner-lane-ms_22358dd14a8d40c9b3a9f60c6e0e5fc2
title: decision
---

Orchestrator corrected heartbeat ownership after Planner forensics: focus must name Orchestrator-owned actions, not wait on Planner. After every accept/close, re-derive dispatch frontier and dispatch/park/replan newly unblocked tasks immediately. A nudge is handled only if it ends in a state change for an open task.
