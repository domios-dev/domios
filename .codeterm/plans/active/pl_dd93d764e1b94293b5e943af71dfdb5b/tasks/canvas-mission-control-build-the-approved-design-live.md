---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_5541cceee3b241569f387389fe944139
handle: canvas-mission-control-build-the-approved-design-live
title: 'Canvas mission control: build the approved design (live)'
status: done
---

## What

Build the approved mission-control canvas (BLOCKED until the design gate task records the user's chosen variant). Replace the section-graph as the canvas primary content; wire the missionControl model live.

## Scope

- Zones layout per the approved variant; agent avatars with the shared liveliness motion language (breathing when Working, slide transitions between stations, reduced-motion static).
- Live updates: model recomputes from existing store updates (PlansProvider + statuses); transitions animate station changes; event particles from new milestones (cap concurrency, drop under load).
- LOD: zoomed-out -> zones + counts; mid -> bays + avatars; close -> full cards. Reuse v2/v3 zoom clamps.
- Interactions: click routes to existing detail surfaces; companion bubble and HUD coexist; keyboard: zone cycling.
- Perf gate: 60fps pan/zoom on a 6-lane busy fixture; no per-frame model rebuilds (memoized selectors); animations CSS/compositor-friendly.
- Playwright visual gate: seeded busy plan screenshots (zones populated, avatar docked) compared in review; e2e click-throughs.

## Migration

Old section-graph canvas retires as the default; keep it behind a view toggle for one release (cheap — it exists), remove in a follow-up once mission control proves out in dogfood.
