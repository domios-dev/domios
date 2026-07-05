---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: section
id: sec_5afc2058f92d4200a5e7e20b018eba1e
handle: w13-active-work
title: Wave 13 — Active-work surfacing everywhere
---

Wave 13 (frontend, after round-2 waves 9-10 land — the strip/badges attach to PlanSwitcher and the v2 list): ambient active-work visibility.

Areas: new shared selector (planView/activeWork.ts) over PlansProvider; PlanSwitcher badges (t9's component); PlanListView pinned strip; CanvasPresenceLayer glow + fly-to-active control (useCanvasFlyTo exists); Sidebar/WorkspacePlanEntry badge+tooltip; Terminal/ComposerPlanActivityStrip current-task line. Status colors from settings TASK_STATUS_COLORS everywhere; stale threshold shared with plan-state drift semantics.
Verification: npm run test:unit (selector + per-surface rendering); Playwright: strip/badges/fly-to specs; screenshots of all six surfaces with live workers for visual acceptance.
