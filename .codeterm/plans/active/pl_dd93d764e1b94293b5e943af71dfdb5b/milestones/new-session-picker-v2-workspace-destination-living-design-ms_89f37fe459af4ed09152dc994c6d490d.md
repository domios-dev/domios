---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_89f37fe459af4ed09152dc994c6d490d
handle: new-session-picker-v2-workspace-destination-living-design-ms_89f37fe459af4ed09152dc994c6d490d
title: decision
---

ROUND-18 HARD REQUIREMENT (user): (1) REUSE our own components — provider/model/workspace selects use src/components/shared/DropdownMenu.tsx (+ Segmented/FloatingTooltip/PairAffordance where they fit), NOT native <select>: the native dropdown flickers and looks foreign. Zero native selects in the picker. (2) Fix the flicker as a root cause (likely re-render churn around the native element — goes away with DropdownMenu, but verify no re-mount loops remain via React profiler or render-count test). Playwright spec addendum: scenario 3/6 selectors switch from select values to DropdownMenu testids; add scenario 11: open each dropdown, assert single render of the menu (no flicker proxy: menu element is not re-created across 10 frames — assert stable element identity or zero mutation-observer churn).
