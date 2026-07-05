---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_5c00ad82793443f180f838d0b5c7523a
handle: t14-active-work
title: 'Active-work surfacing: strip, badges, glow, sidebar, composer'
status: done
---

Ambient active-work visibility across all six surfaces (spec section active-work; wave w13-active-work). Depends on t9-board-v2 (PlanSwitcher) and t10-list-v2 (v2 stream) landing first.

Steps:
1. Shared selector planView/activeWork.ts over PlansProvider: in_progress/in_review tasks + executor presence (pane alive) + freshness (latest stride/report ts) + plan ref; staleness threshold shared with plan-state stale_in_review semantics. Unit-test the selector; every surface below consumes it (no re-derivation).
2. PlanSwitcher: per-plan in-flight badge (count), tooltip with task titles.
3. PlanListView: pinned "In flight" strip above the stream (one line per task: status, name, executor, freshness; click -> detail overlay); visible regardless of filters.
4. Canvas: presence-consistent glow/pulse on active task nodes; "fly to active" control cycling in-flight nodes via useCanvasFlyTo.
5. Sidebar WorkspacePlanEntry: in-flight count badge + tooltip (titles/executors), live-pane indicator.
6. ComposerPlanActivityStrip: current task of THIS pane's agent (name + status pill).
7. Stale in-flight renders visually distinct (dimmed pulse / warning tint) on every surface — forgotten work must be visible.
8. Consistency: TASK_STATUS_COLORS + shared click-through everywhere; testids: active-work-strip, plan-switcher-badge, fly-to-active, sidebar-inflight-badge, composer-current-task.
9. Tests: selector unit tests; per-surface rendering tests; Playwright specs for strip/badges/fly-to; screenshots of all six surfaces with live workers.

Commands: npm run test:unit; npx playwright test -c tests/playwright --project=golden; dev screenshots in report.
Report: selector semantics, per-surface screenshots, testids, staleness threshold chosen.
