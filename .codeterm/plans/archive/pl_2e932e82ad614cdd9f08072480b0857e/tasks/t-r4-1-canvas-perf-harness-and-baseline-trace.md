---
plan_id: pl_2e932e82ad614cdd9f08072480b0857e
kind: task
id: task_1cff17806abd4bafbc9c56d471ddd72c
handle: t-r4-1-canvas-perf-harness-and-baseline-trace
title: T-R4.1 — Canvas perf harness and baseline trace
status: cancelled
tags:
- canvas
- performance
- playwright
- substrate
---

Build the measurement harness before more renderer changes.

Goal: capture reliable local evidence for the current canvas jank with substrate flags on/off.

Scope:
- Add a Playwright perf spec (for example tests/playwright/suites/13-canvas/canvas-perf.spec.ts) that uses deterministic 100/500/1000 task fixtures.
- Measure rAF frame intervals during pan/zoom, long tasks through PerformanceObserver, world-layer DOM node count, CanvasView/render counters if feasible, overlay budget, and canvas nonblank pixels.
- Run both default flags and substrate flags: canvas.passiveRenderer=true, canvas.substrateV2=true, canvas.overlayBudget=true.
- Keep product changes limited to test-only instrumentation or a tiny dev-only render counter hook if necessary.

Acceptance gates:
- Baseline output reports median/p95 frame time, longtask count, DOM node count, overlay count, and canvas nonblank state for 100/500/1000 nodes.
- The spec is repeatable locally against a separate dev instance or clearly documents how to avoid focus fights on 7686.
- The result can fail as a performance budget only after we choose thresholds; initial task may record metrics without gating the whole suite.
