---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_c9cf272cb59f489db358c6aeb08f0f13
handle: orchestrator-creation-route-hardening
title: Orchestrator creation route hardening
status: done
---

Matrix task task_1800fa99 is in_review but currently FAILS on product behavior. Fix the route gaps; do not edit the matrix to hide failures.

Current matrix findings against dev 35738df5:
- C role spawn: spawned pane exists, but onboarding does not carry the peer-pairing mandate block. Planner spawn command is not pre-filled with the real parent/orchestrator pane id.
- D promotion: role/session update works, but promotion onboarding/mandate is not visible in the pane chat; command/RPC response does not carry the required mandate block.
- G doctor ladder: plan doctor --kind quality has no planner_unpaired warning/error ladder for unpaired orchestration plans; it emits only missing_goal/missing_spec_source.
- H1 negative: agent spawn --role Orchestrator without --workspace falls back to active workspace instead of failing explicitly.
- H2 negative: no backend hierarchy guard prevents a Worker-attributed orchestration spawn / orchestrator creation route.

Fix shape:
1. Make role-spawn and promotion paths deliver or return the planner-pairing mandate in the route the matrix expects. If the product intentionally uses pane chat instead of command response, update the command response with a concise pointer plus ensure the pane transcript is observable without timers.
2. Add planner_unpaired quality doctor ladder: warning in early planning/unapproved state, error when execution/approval requires pairing, silent after planner bound or explicit waiver with note.
3. Refuse Orchestrator role spawn without explicit workspace unless an existing orchestration context makes the target unambiguous; the current active-workspace fallback is too dangerous.
4. Add backend guard: Workers cannot create/promote/spawn Orchestrator routes. Prompt-only is insufficient.
5. Keep pair creation and normal Planner binding working.

Coordinate with taxonomy task for planner onboarding wording; do not modify sidebar taxonomy files unless strictly necessary.
