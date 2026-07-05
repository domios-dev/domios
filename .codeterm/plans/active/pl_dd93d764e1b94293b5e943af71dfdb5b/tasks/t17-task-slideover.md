---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_f98cc5ef604f456591e74ebede558bf0
handle: t17-task-slideover
title: Task slide-over panel (right, live left surface)
status: done
---

Task slide-over panel (round 4). Read the ROUND 4 block of the detail-surface spec section first.

Steps:
1. New PlanTaskSlideOver: right panel ~40-45% width, independent scroll, Esc/X close, focus-trapped but LEFT SURFACE STAYS LIVE — clicking another task/card updates the panel in place (no close/reopen flicker).
2. Task kind routes to the slide-over from Board cards, List rows, Canvas chips; plan/spec kinds KEEP the fullscreen document style (they are documents).
3. Inside: existing task detail content + linked plan sections as navigable links (-> plan document at section, t18 route) + attachments thumbnails (t15 hook, degrade gracefully).
4. Tests: slide-over open/update/close, left-surface interactivity while open, kind routing; Playwright spec; screenshots.
