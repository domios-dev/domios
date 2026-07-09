---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_2d6065f2657347849914b008beb83c3f
handle: sidebar-taxonomy-role-badges-managers-workers-sections-planner-onboardin-ms_2d6065f2657347849914b008beb83c3f
title: testing
---

BOUNCE FINDINGS (round-32, user screenshot of the landed state): (1) ROLE BADGE DOES NOT FIT — 'ORCHESTRAT…' truncates in the bubble; size the badge to the full word (smaller type/tracking or wider chip; 'Orchestrator' must render whole — truncated role labels defeat the entire purpose). (2) PLANNER HAS NO BADGE and (3) IS FILED UNDER WORKERS: the pane whose chat literally introduces itself as the Planner (under Orchestrator Byron) renders in the WORKERS section with no badge. ROOT CAUSE is backend: the planner is structurally AgentRole::Worker with the planner role_profile, so any mapping keyed on AgentRole alone classifies it as Worker. FIX: the role->section module must key on (role, role_profile, planner_ref match) — planner_ref binding OR role_profile=planner => Manager section + PLANNER badge; expose whatever field the frontend is missing in the pane snapshot (backend half of this lane). (4) CONTAINMENT UNREADABLE: managers/workers do not visually belong to their orchestrator — the sections must render NESTED under the owning orchestrator's tab (indent + the group's left containment line), not as flat global sections; with two orchestrators on screen (Main + Byron) flat sections are ambiguous by construction. Acceptance addition: a pair + one worker under Byron renders as Byron > Managers > [Kelvin PLANNER] / Workers > [worker], visually contained, while Main's group renders separately; badges never truncate at any of our three UI font scales.
