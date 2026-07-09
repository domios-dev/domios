---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_57fc334a407b4770ad134e2b362c296d
handle: planner-lane-ms_57fc334a407b4770ad134e2b362c296d
title: decision
---

STALL FORENSICS + PROMPT FIX. Timeline (milestone ts): 02:28 Puccini accepted Canvas v3 — t7-review's precondition became TRUE at that moment — then wrote heartbeat focus 'Next: planner-lane decides whether to dispatch…' and went dead 02:28-07:51 (5.4h; resumed only on my round-4 report event). Root cause: OWNERSHIP-INVERTED FOCUS — heartbeat prepends focus to every nudge, so a waiting-on-planner focus converted each nudge into confirmation of idleness; nothing in docs/prompts forbade it, and nothing demanded frontier re-derivation after a gate. (Night machine-sleep may have eaten some nudges — secondary; the waiting decision predates it. t19 escalation covers post-wake catchup.) FIXES SHIPPED (uncommitted, integration checkout): docs/ORCHESTRATION.md rev18 adds 'Focus discipline' + 'Dispatch frontier' subsections; src-tauri/prompts/orchestrator_suffix.md + orchestration_toolset.md carry the same contract (needs dev rebuild to reach future spawns); ~/.codeterm/skills/codeterm-orchestrator/SKILL.md gains loop items 6-7 (focus ownership, frontier re-derivation). t19's relentless-heartbeat remains the enforcement layer; these are the prompt layer.
