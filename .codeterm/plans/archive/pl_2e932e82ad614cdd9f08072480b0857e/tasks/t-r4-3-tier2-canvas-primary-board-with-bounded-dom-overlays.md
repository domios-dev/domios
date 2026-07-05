---
plan_id: pl_2e932e82ad614cdd9f08072480b0857e
kind: task
id: task_6f6bbab91eef413b86b3ec2e5573b619
handle: t-r4-3-tier2-canvas-primary-board-with-bounded-dom-overlays
title: T-R4.3 — Tier2 canvas-primary board with bounded DOM overlays
status: cancelled
tags:
- canvas
- performance
- implementation
- substrate
---

Tier2 implementation from R4, after Tier1 measurement.

Goal: make canvas the primary base renderer and bound DOM to a small overlay pool.

Scope:
- Promote passive renderer from additive backdrop to primary base board renderer for non-focused nodes.
- Replace the visible world DOM node list with a bounded lightweight/overlay pool; base node DOM count must not scale with task count.
- Keep rich DOM overlays only for selected/focused/deep nodes, with budget <= 2-3.
- Maintain focus-by-id, hit testing, visual correctness, and accessibility fallback for active objects.

Acceptance gates:
- At 1000 tasks, world-layer DOM node count remains bounded regardless of zoom density.
- Canvas nonblank and hit/focus behavior pass Playwright checks.
- Rich overlay DOM count stays within budget.
- Median frame metrics improve versus T-R4.1/Tier1 baseline.
