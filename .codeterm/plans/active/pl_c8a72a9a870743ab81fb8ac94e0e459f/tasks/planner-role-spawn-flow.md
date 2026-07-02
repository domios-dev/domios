---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_216e66dfebef4bd4ade8012250c546d3
handle: planner-role-spawn-flow
title: Implement role-aware Planner spawn/onboarding flow
status: done
tags:
- 1.6.2
- followup
---

Implement the adopted design in plan section sec_553bdf825b194a2797e54abdd6eb3b94 (Planner role — adopted design). Backend lane, all src-tauri. Runs FIRST (before task_ffddc406 — both touch prompts/onboarding text).

1. `Pane.role_profile: Option<String>` — multiplexer/types.rs + snapshot + closed store, serde default. No migration needed.
2. Spawn path: `role_profile` on the agent spawn spec + REST/RPC route; CLI sugar `agent spawn --role Planner` maps to role=Worker + role_profile="planner" at the CLI/route boundary. Service `parse_role` (service/mod.rs:312) stays two-variant. Unknown profile: log warn + proceed as generic Worker.
3. New `src-tauri/prompts/planner_suffix.md` (draft body is in the design section) + prompts.rs const. Extend `role_onboarding_task` (service/orchestrator.rs:664) to take the profile: (Worker, "planner") -> PLANNER_SUFFIX; existing behavior otherwise. Update orchestrator/tests.rs.
4. Task DTO `role_guidance: Option<String>` (plan/types.rs) + add/update mutations + CLI `plan task add/update --role-guidance` / `--role-guidance-file` + printed by `task show` + prepended to the worker brief at dispatch/respawn composition (same point as the worker preamble).
5. `TaskExecutionAttempt.role` (existing free-form field) records "planner" when the executor pane has that profile.
6. Tests: spawn sugar mapping, suffix selection by profile, role_guidance round-trip (add/update/show/dispatch prepend), unknown-profile warn. Respect file-size limits; no builds on the weak machine — orchestrator gates.
