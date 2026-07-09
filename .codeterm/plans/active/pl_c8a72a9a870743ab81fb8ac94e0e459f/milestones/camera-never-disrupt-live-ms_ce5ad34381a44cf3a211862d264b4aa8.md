---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_ce5ad34381a44cf3a211862d264b4aa8
handle: camera-never-disrupt-live-ms_ce5ad34381a44cf3a211862d264b4aa8
title: decision
---

ANDREY (hard rule §31): the board is live (workers add tasks/strides, plans mutate) but NONE of it may EVER move the user camera/zoom/focus. Programmatic/remote change = node updates IN PLACE, zero camera motion. User-initiated motion (minimap click, Plan/show-me button, their own canvas focus, drill, zoom/fit) is fine. Agent/CLI canvas focus = OFF-SCREEN SIGNAL (edge dot/glow toward the change + optional count, click to fly), NOT auto-pan. Applies to dominance(§25.3)/handoff(§27.4)/status(§25.6)/auto-pack. Playwright: after a live mutation, camera transform byte-identical.
