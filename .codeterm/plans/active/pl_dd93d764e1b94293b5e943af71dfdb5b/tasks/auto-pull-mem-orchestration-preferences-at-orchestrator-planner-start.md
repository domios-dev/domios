---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_5939a1535b4347c59e671ef209073a9c
handle: auto-pull-mem-orchestration-preferences-at-orchestrator-planner-start
title: Auto-pull mem orchestration preferences at orchestrator/planner start
status: pending
---

ANDREY'S REQUIREMENT (round-78): machine/project-specific orchestration knowledge lives in mem as DELTAS over the canonical embedded docs — but deltas only work if they arrive AUTOMATICALLY at start; today the prompts merely instruct agents to search mem ('check mem wake-up / search orchestration preferences'), which is hope, not mechanism (the shipped-but-unadopted lesson applies to knowledge too). MECHANISM: at orchestrator promotion and pair-spawn onboarding (orchestrator_onboard.rs / peer greeting assembly), the daemon queries mem (kind=preference, area=orchestration, machine scope + the workspace's project scope, limit ~5, importance-ranked) and appends a compact block to the onboarding text: 'Machine/project orchestration preferences (from mem): - <title>: <one-line body> ...'; empty result = zero lines (silent). Same for the Planner's onboarding (planning preferences). BOUNDED: the mem query must never delay spawn — timeout -> skip with a log line; total block <=8 lines (truncate by importance). PRODUCT-GENERAL: kind/area conventions documented in docs/ORCHESTRATION (the dedupe task adds the note); no machine names in code. This closes the loop the orchestrator-skill dedupe opens: canonical discipline in embedded docs, machine deltas in mem, deltas auto-injected — three layers, no duplication.
