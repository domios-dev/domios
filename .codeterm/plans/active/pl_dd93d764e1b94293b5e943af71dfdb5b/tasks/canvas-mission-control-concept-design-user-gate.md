---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_4d59bc4771f74be28ea6fbcadde5a4fe
handle: canvas-mission-control-concept-design-user-gate
title: 'Canvas mission control: concept design + user gate'
status: done
---

## What

Design the Canvas mission control concept to a user-approvable state: interactive prototype (real code, seeded data, no live wiring yet) + screenshots. HARD GATE: the user approves the concept before any build task starts. This resumes the round-3 spec direction note ("mission control, do not implement from the note alone") — the note is the brief, this task is the design.

## Concept to develop (starting point, iterate)

Canvas stops being a section graph (Board/List own the document structure). It becomes the OPERATIONS ROOM of the plan:

- **Zones (spatial stations):** Dispatch frontier (pending unblocked tasks as a queue rail) -> Work floor (center: one bay per live lane — task card + its worker avatar) -> QA bay (queue + the single running slot, from the QA pipeline) -> Gate desk (orchestrator judging in_review) -> Done shelf (compressed, count-first). Parked/blocked side rail.
- **Agents as presences:** every live pane (orchestrator, planner, workers, QA tester) is an avatar (provider glyph + presence color + status treatment shared with sidebar liveliness). Avatars are AT their station: worker avatar docked to its lane bay; on report the avatar slides to the gate desk; QA claim moves it to the QA bay. Movement = the life the user wants; slow eased transitions, never teleports, reduced-motion static.
- **Event stream:** working points/reports render as small ephemeral particles/toasts drifting from the emitting avatar toward the plan HUD (last ~5 visible, click -> task detail).
- **Plan HUD (corner):** progress %, live lanes count, QA queue depth, loops (running/budget), competitive attempts, cleanup debt count. Companion bubble (Wave 20) coexists.
- **Interaction:** click task/avatar/particle -> existing slide-over/detail surfaces (no new inspectors); zoom clamps + LOD from Canvas v2/v3 retained (zoomed out: zones + counts only).

## Deliverables

1. 2-3 layout variants (zone arrangement, density) as coded prototype states with seeded fixture data (a plan with 6 lanes, QA queue of 3, 1 loop, 1 competition).
2. Screenshots of each variant at 100% and zoomed-out LOD, dark theme, attached to the report.
3. A written motion spec (what moves when, durations, reduced-motion behavior) as a short section in the task report + spec section update draft.
4. User gate: present variants -> user picks/edits -> record the decision as a working point; only then the build tasks unblock.

## Out of scope

Live data wiring, performance work beyond reusing existing clamps, editing/canvas-notes features.
