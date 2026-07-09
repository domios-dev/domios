---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_115a7c011518406ea7ba9a339a773a47
handle: plan-companion-bubble-talk-to-the-planner-from-plan-views
title: 'Plan companion bubble: talk to the Planner from plan views'
status: done
---

## What

Plan views get a companion bubble: a floating avatar (bottom-right of the plan surface) representing the live PLANNER pane of this plan (fallback: the Orchestrator when no planner pane exists). Click opens a compact popover chat with that pane — the user talks to the plan's brain without leaving the plan.

## Design

- Bubble: pane presence color ring + provider glyph + live status (reuse the sidebar liveliness treatment language: breathing when Working). Tooltip: "Planner — <pane title> (<status>)".
- Click -> anchored popover (~380px wide, ~60% height) embedding the existing pane chat surface (reuse PaneSessionChat) with a header (name, status, open-full-pane button that jumps to the pane and closes the popover). Esc closes. One companion popover at a time.
- Pane resolution: plan's planner pane binding when the peer-model lands (Wave 21); until then resolve: pane assigned to an open planning-kind task, else the plan's orchestrator_ref pane. Pure resolver, unit-tested, upgrade path documented in code.
- Bubble hidden when no live pane resolves (no dead-avatar clutter). prefers-reduced-motion: static.

## Files

New PlanCompanion component under planView; resolver module; wiring into PlanViewShell (all three views); reuse PaneSessionChat.
