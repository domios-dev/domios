---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_7d28a708a5aa495d94ed098d8959d11b
handle: group-containment-one-visual-block-per-orchestration-p0-split-ms_7d28a708a5aa495d94ed098d8959d11b
title: decision
---

SCOPE REFINEMENT (round-43, user, verbatim intent): the orchestration group must collapse LIKE A WORKSPACE — a neat, pleasant handle on the RIGHT side of the group head row, mirroring the workspace collapse affordance; default state EXPANDED. Current code already has the mechanism (SidebarWorkspaceTabs.tsx collapsedGroups state, TabItem.tsx group-collapse-toggle, collapsedSummary '1 manager · 2 workers') but renders a 12px chevron on the LEFT before the provider glyph — wrong side, wrong feel. Required: (1) move the toggle to the RIGHT edge of the orchestrator head row, visually matching the workspace handle (same size/color/rotation animation family as WorkspaceItem.tsx chevron: muted, hover brightens, 320ms cubic-bezier rotate); (2) default expanded stays (new Set() — already correct); (3) collapsed head keeps the summary line; the containment hairline and children disappear as one unit; (4) keep data-testid group-collapse-toggle so existing GroupContainment tests survive — extend them with a right-side position assertion (toggle is the LAST interactive element in the head row) and a default-expanded assertion. This folds into the same elegance pass — one lane, not a second task.
