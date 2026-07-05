---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_bb29cfe0b4a34ff18033191abb0725c6
handle: plan-picker-per-plan-kebab-menu-archive-rename-more
title: 'Plan picker: per-plan kebab menu (archive, rename, more)'
status: done
---

## What

Add a per-plan overflow (kebab, three-dots) menu to the plan picker used by Board and List views (`src/components/Canvas/planView/PlanSwitcher.tsx`), giving users plan management actions without leaving the view.

## Design spec (frontend-designer stance)

**Affordance.** Each plan entry in the picker gets a kebab trigger to the right of the title. It is quiet by default and appears on hover/focus of the entry (always visible on touch/coarse pointers). The kebab click must NOT change plan selection — separate hit target, stopPropagation.

**Two picker modes, one menu.**
- Segmented mode (<=3 plans): the current segment is a single `<button>`; button-in-button is invalid HTML. Restructure each segment into a group wrapper: [select surface][kebab button], visually one pill. Active/selected styling stays on the wrapper.
- Overflow mode (>3 plans): native `<select>` cannot host row actions. Replace with a custom popover listbox (same trigger styling as today): each row = plan title + freshness hint + kebab. Full keyboard support: Up/Down navigate, Enter selects, ArrowRight or Shift+F10 opens the row menu, Esc closes (menu first, then list). Focus returns to the trigger on close.

**Menu actions (v1).**
1. **Rename…** — inline text field in a small dialog, saves via PATCH `/api/plans/{id}` (`title`). Disabled while a rename is in flight.
2. **Copy plan id** — clipboard + toast with the short id.
3. **Open plan document** — deep-link to the plan document surface (Wave 14, task_8be0a8a58f5342659e2de4a756650daf). If that surface has not shipped yet, hide the item (capability check), do not show a dead entry.
4. **Archive…** — destructive-styled, separated by a divider, confirm dialog naming the plan ("Archive '<title>'? Workers keep their history; the plan disappears from pickers."). POST `/api/plans/{id}/archive`. If the archived plan was selected, auto-select the freshest remaining plan; if none remain, show the existing empty state.
5. **Show archived…** (list footer item, not per-plan): reveals an "Archived" group at the bottom of the popover with per-row **Unarchive** (POST `/api/plans/{id}/unarchive`). Archived rows are visually muted and not selectable as the active plan.

**States.** Optimistic UI with rollback on API error + toast. While a mutation is pending, the row shows a spinner in place of the kebab. Menu is a single shared component — Board and List must render identical menus (List v4 task_168db0b723d04d72a0e5007d3d9464e1 mounts the same PlanSwitcher; coordinate, do not fork).

**A11y.** Trigger: `aria-haspopup="menu"`, `aria-expanded`. Menu roles: `menu`/`menuitem`. Announce result of archive/rename via live region (existing toast system if it already does this).

## Out of scope

Plan delete (archive is the safe verb; no destructive delete in the picker), approve/unapprove (orchestration surface, not picker), mesh-remote plans.

## Files

`src/components/Canvas/planView/PlanSwitcher.tsx` (+ tests), `PlanBoardView.tsx`, `PlanListView.tsx`, styles in the plan-view stylesheet; API client additions for archive/unarchive/rename if not present in `src/lib`.
