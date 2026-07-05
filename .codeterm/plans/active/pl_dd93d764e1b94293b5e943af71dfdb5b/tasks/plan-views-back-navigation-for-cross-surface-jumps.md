---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_f833b4758ddd47ada0afebbd5b5dbbbf
handle: plan-views-back-navigation-for-cross-surface-jumps
title: 'Plan views: back navigation for cross-surface jumps'
status: done
---

## What

Link-jumps inside plan surfaces (task detail -> plan section, section -> spec section, activity click-throughs, coverage links) are one-way: there is no way back. Add a navigation history with a back button.

## Design

- A per-plan-view navigation stack (pure module, unit-tested): every programmatic surface jump pushes {surface, id, scroll}. Back pops. Forward optional (stack only, no forward, keeps UX simple).
- Back affordance: a compact chevron-left button in the detail overlay / slide-over / section view header, visible only when the stack is non-empty; keyboard: Alt/Cmd+Left and Esc falls back to close-current as today (Esc semantics unchanged).
- Breadcrumb (optional if cheap): "Task > Wave 13 > Spec: active-work" as muted trail in the header, each crumb clickable (equivalent to popping to that depth).
- Stack resets on plan switch; capped (~20 entries); restores scroll position of the returned-to surface.

## Files

New src/components/Canvas/planView/navStack.ts (+tests); detail overlay, slide-over, spec/section view headers wire push/pop.
