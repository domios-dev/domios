---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: section
id: sec_5049d213a0a046f39c98ea7aadf6b3de
handle: canvas-performance-stability
title: Canvas performance & stability
tags:
- performance
- stability
---

Make the canvas comfortable under real orchestration load. Working agents must not make the board lag. Presence pins/connectors must move with the same camera transform as nodes and must never leave orphan dotted lines. Adding/updating tasks must preserve unrelated node world positions and must not auto-refit/jitter the camera. Acceptance requires focused perf/stability tests plus Playwright/manual dev verification on the real dogfood plan.
