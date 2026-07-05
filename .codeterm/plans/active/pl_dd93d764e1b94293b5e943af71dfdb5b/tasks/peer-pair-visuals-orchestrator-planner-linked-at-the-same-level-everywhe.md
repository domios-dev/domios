---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_975ca75247ef47f5bb5855ff7d4feafb
handle: peer-pair-visuals-orchestrator-planner-linked-at-the-same-level-everywhe
title: 'Peer pair visuals: Orchestrator + Planner linked at the same level everywhere'
status: done
---

USER HARD REQUIREMENT (stated twice): the Orchestrator and its Planner are peers — every surface must render them as a visually LINKED PAIR at the SAME level, never planner-nested-under-orchestrator. Surfaces to fix/verify: (1) Sidebar HierarchyTree — planner pane currently renders as a CHILD row under the orchestrator (parent_pane_id); render the pair as SIBLINGS at the same indent joined by a pair affordance (shared bracket/link glyph on the left edge, or a two-row pill group), workers remain children below the pair; (2) plan header peer chips (landed in planning-state lane) — verify equal-size chips joined visually, adjust if orchestrator chip dominates; (3) New-session pair card (coordinate with the picker task — same pair affordance component); (4) companion bubble tooltip naming both peers equally. Extract ONE PairAffordance/PeerPairRow component used by all surfaces. Component tests: planner is sibling not child in the tree, pair affordance present, workers still nested under the pair. Screenshot evidence of all four surfaces in report.
