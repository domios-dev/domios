---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_ba611044538a4ef2a6dc6db508b7a7b8
handle: task-lane-watchdog-tie-in-progress-truth-to-live-pane-mesh-state-ms_ba611044538a4ef2a6dc6db508b7a7b8
title: review
---

RETRO — why non-QA panes still built AFTER the agreement (round-70, Orchestrator self-ask, honest answer): (1) THE AGREEMENT WAS CHAT — the interim rule lived in messages to you, not in any worker's spawn prompt: every worker spawned before (and even after) the rule was born with the OLD worker_suffix that literally demands BUILD/TYPECHECK/TESTS results in reports — the form kept asking, so workers kept building; a rule stated to the orchestrator does not propagate to already-alive workers or to templates. (2) NOTHING WATCHED — build-detection (5a6d58c5) is filed, not landed; between agreement and enforcement there is a window where only memory holds, and memory loses under load — the third occurrence of the same meta-lesson. (3) YOUR QA RUNNER COEXISTS with capable workers: registering b813bb47 as the runner changed nothing about what OTHER panes could do. ENFORCEMENT THAT CLOSES IT (all filed): QA-pane registry singleton + registry-keyed build detection (5a6d58c5, sharpened this round), rewritten report template (same task), spawn guard (46d7cc4c). FUTURE-PLANNER GUARANTEE: these land in product code + wiring-tested templates — a future agent cannot repeat this by forgetting, only by explicitly recorded exception. Interim until 5a6d58c5 lands: when dispatching ANY worker, prepend the code-only line in the task prompt itself (not chat), since the template still asks for builds.
