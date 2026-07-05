---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_769e9c0cfcdf4e5f87907fcbf978697f
handle: group-containment-one-visual-block-per-orchestration-p0-split
title: 'Group containment: one visual block per orchestration (P0 split)'
status: done
---

## What

P0 SPLIT from the taxonomy lane (third occurrence of the containment finding — per the bounce discipline, it becomes its own task with an exact spec instead of a third identical bounce). Evidence (round-35 screenshot): two orchestration groups render FLAT and interleaved — "Planck / MANAGERS / Planck / Euler / MANAGERS / Planner" — adjacent groups visually merge, nothing shows whose managers are whose, badges still truncate, and planner NAMES collide (one planner named identically to its orchestrator, another literally "Planner").

## EXACT VISUAL SPEC (implement this drawing, not an interpretation)

```
▾ ✳ Planck                    ORCHESTRATOR · OPUS
┃   MANAGERS
┃     ✳ Kepler                PLANNER · FABLE
┃   WORKERS
┃     ✳ Bohr                  SONNET
┃     ✳ Curie                 SONNET
╵── 8px gap ──────────────────────────
▾ ✳ Euler                     ORCHESTRATOR · OPUS
┃   MANAGERS
┃     ✳ Noether               PLANNER · FABLE
```

1. **One visual block per group.** A continuous left containment line (2px, the orchestrator's presence color at ~35% alpha) runs from under the orchestrator row to the group's last child. Children indent 14px inside it. The line IS the ownership statement — an eye following it up always lands on the owner.
2. **Section headers live INSIDE the container**: MANAGERS / WORKERS indented at child level, 9px uppercase muted — never full-width flat rows that read as global dividers.
3. **Group separation:** 8px vertical gap between groups (optional hairline). Two adjacent groups must be visually impossible to misread as one.
4. **Group head = collapse control.** Chevron on the orchestrator row collapses/expands the whole group (children + section headers); collapsed head shows a compact count ("1 manager · 2 workers").
5. **Badges fit whole — hard criterion, third strike.** ORCHESTRATOR/PLANNER + model render untruncated at ALL THREE UI font scales; component test measures scrollWidth<=clientWidth on the badge at each scale. If the combined form cannot fit at the smallest scale, the model drops from the badge at that scale ONLY (role always wins) — deterministic rule, not truncation.
6. **Planner naming (backend, pair-spawn path):** the planner gets its OWN generated agent name like any spawned agent — never a copy of the orchestrator's name, never the literal string "Planner" (the badge carries the role; the name is identity). Fix wherever pair creation derives the planner pane title.

## Executable test plan

1. Component: seeded two adjacent groups -> assert containment line per group (testid group-container per orchestrator), children indented, section headers inside; snapshot at all three font scales.
2. Component: badge overflow assertions (scrollWidth<=clientWidth) for ORCHESTRATOR·OPUS and PLANNER·FABLE at 3 scales; smallest-scale fallback drops model, keeps role.
3. Component: collapse -> children+headers hidden, head shows "N managers · M workers"; expand restores.
4. Backend unit: pair spawn derives distinct planner name (not orch name, not "Planner") — name-generator path test.
5. Live dev: spawn two pairs, screenshot matches the spec drawing (attach side-by-side with the drawing).
