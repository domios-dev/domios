---
plan_id: pl_2e932e82ad614cdd9f08072480b0857e
kind: task
id: task_3d7b1860732047f5b27a8bfd96bcf935
handle: t1-figma-canvas-acceptance-harness
title: T1 — Figma-canvas acceptance harness
status: done
tags:
- canvas,figma-substrate,playwright
---

Build the acceptance harness for the Figma-grade canvas migration before replacing rendering behavior.

Scope:
- Add deterministic large-canvas fixtures for 100/500/1000 task worlds using existing plan/layout inputs.
- Add reusable Playwright helpers for world-box vs screen-box assertions across zoom/pan.
- Add a canvas pixel nonblank helper for renderer layers.
- Add overlay budget assertions that count rich DOM card mounts under a feature flag.
- Document exact commands in the test file header or helper README if needed.

Acceptance:
- Tests are additive and do not require the new renderer to be enabled by default.
- Helpers can assert: fixed geometry across W0-W3, nonblank canvas layer, focus-by-id target, DOM overlay count.
- Run npx tsc --noEmit and the focused Playwright/helper tests in hot checkout before close.
