---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_10d6f235e8434acb99a75711478afb1e
handle: sidebar-plan-entry-as-native-workspace-surface-single-selection
title: 'Sidebar: plan entry as native workspace surface (single selection)'
status: done
---

## What

Sidebar plan entry behaves half-tab, half-widget: it can be highlighted simultaneously with a session tab (user screenshot: plan entry AND a session tab both selected), and visually it reads as a foreign element. Make the plan a first-class workspace surface.

## Design

1. **Single selection invariant.** Within a workspace exactly ONE sidebar item is selected: a session tab OR the plan entry. Selecting the plan deselects the tab highlight (the underlying pane keeps running, only selection/иighlight moves); selecting a tab deselects the plan. Selection = what the main area shows.
2. **Native-but-distinct styling.** The plan entry uses the tab row grammar (same height, padding, hover, active pill) but differentiated as a workspace-level surface: plan glyph, slightly distinct tint (e.g. accent-tinted background at rest), positioned pinned at the top of the workspace's tab group. It must read as "belongs to this workspace" — not as another agent.
3. Badge behavior stays (in-flight count from activeWork selector), title truncation fixed to word-boundary.
4. Keyboard: plan entry participates in tab cycling (Ctrl+Tab order) as the first element of its workspace.

## Files

src/components/Sidebar/WorkspacePlanEntry.tsx, Sidebar selection state (Sidebar.tsx / store), TabItem active-pill logic, styles.
