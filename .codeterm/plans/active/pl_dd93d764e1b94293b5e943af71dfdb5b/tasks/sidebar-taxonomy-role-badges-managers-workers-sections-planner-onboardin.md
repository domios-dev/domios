---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_25633971d82349eda326bd9699b3e151
handle: sidebar-taxonomy-role-badges-managers-workers-sections-planner-onboardin
title: 'Sidebar taxonomy: role badges + Managers/Workers sections + planner onboarding'
status: done
---

## What

Round-21 sidebar taxonomy for orchestration groups — makes the subordination model VISIBLE and simplifies the code. Three user-verified pains: (1) tabs don't say who is the Orchestrator and who is the Planner; (2) the Planner receives no onboarding message (the orchestrator gets one, the planner boots into silence); (3) the pair still renders same-level.

## Target structure (sidebar orchestration group)

```
[Orchestrator tab]            <- group head; badge says ORCHESTRATOR
  Managers                    <- section header (tiny, muted, uppercase)
    [Planner tab]             <- badge says PLANNER
    (future: watcher tabs — monitoring/oversight panes join here)
  Workers                     <- section header
    [worker tabs]             <- unchanged rows
```

## Changes

1. **Role badges replace model badges for the pair.** Orchestrator and Planner tabs show ORCHESTRATOR / PLANNER in the bubble where the model chip sits today; the model moves to the tooltip (and stays in pane detail). The orchestrator STAR is deleted — the badge carries the meaning now (ROLE_ICONS star/gear cleanup coordinates with the sidebar-liveliness task which already removes the gear). Workers keep their model chip; their section header already says Workers.
2. **Section taxonomy inside the group.** A pure role->section mapping: Manager = {Planner, future watcher roles}, Worker = {Worker}. Section headers render only when the section is non-empty. Mapping lives ONCE (single module, unit-tested) — the sidebar, group-close confirm dialog ("closes: orchestrator + 1 manager + 3 workers"), and any future surface consume it (DRY).
3. **Planner onboarding message.** On pair spawn AND on planner bind (spawn --role Planner --plan), the planner receives a first message naming its role, its orchestrator (pane id + name), and its charter — reuse the subordination task's rewritten constants (planner works under the orchestrator's direction). The planner must never boot into silence.
4. **Code simplification dividend** (user's explicit bet): the role->section function replaces scattered role checks in Sidebar/HierarchyTree; star special-casing deleted; expect net-negative or neutral diff outside the new module.

## Sequencing

Depends on task_831245e5 (subordination — supplies the constants and kills peer-equality rendering) and coordinates with task_e0ddee08 (group v2, in_review — same sidebar region; land taxonomy AFTER its merge to avoid conflicts).

## Executable test plan

1. Component: seeded group (orch + planner + 2 workers) renders section headers Managers/Workers, planner first under Managers; badges read ORCHESTRATOR/PLANNER (testids role-badge-orchestrator, role-badge-planner, group-section-managers, group-section-workers); no star glyph remains (assert absent).
2. Component: group with no workers renders NO Workers header; solo orchestrator (waived planner) renders no Managers header.
3. Unit: role->section mapping matrix incl. unknown-role fallback (goes to Workers with a debug log, never crashes).
4. Live dev: spawn pair via picker -> screenshot the group; planner chat shows its onboarding message naming the orchestrator (transcript in report).
5. Group-close confirm lists counts by section ("1 manager, N workers") — component test.
