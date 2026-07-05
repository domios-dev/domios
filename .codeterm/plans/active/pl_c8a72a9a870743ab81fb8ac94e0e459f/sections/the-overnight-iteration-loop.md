---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: section
id: sec_f85f2c72f6fd41c881ca8ebde6eb45af
handle: the-overnight-iteration-loop
title: The Overnight Iteration Loop
---

**HOW the board was driven from "correct" to "Figma-grade."** This is a process, not a module — but it's a reproducible one with real harness artifacts. The board was polished over ~10 build iterations, each design-reviewed by a *fresh independent critic* using the `frontend-design` skill, scored against a 10-dimension rubric. Roles: a lead designer-engineer (build), an adversarial critic (score + critique), a mechanical fixer (apply). The cycle:

```
build → integrate → per-zone Playwright screenshot → independent score → fold critique → repeat
```

The per-zone screenshots are real and committed under `tests/playwright/artifacts/canvas/` (e.g. `layout-integrity-1.75-canvas-plan-node.png`, `…-canvas-spec-node.png`, `…-canvas-task-card.png`) — far/mid/near × per-zone, so a critic scores the *actual* render at each distance, not a description. Scale is exercised separately by the perf harness: `perfFixture.ts` injects a synthetic N-task plan client-side (`localStorage["canvas.perfFixtureN"]`), and `wk-perf-probe.mts` samples rAF frame intervals (p50/p95/dropped%) during idle/pan/zoom so each iteration's smoothness is a number, not a vibe.

The loop's value is the independent critic catching over-corrections a single builder can't self-see — edges swinging loud↔invisible, the spec node demoted↔chromeless — because the critic starts fresh each round with no attachment to the previous fix.

## Key decisions
- **Fresh adversarial critic each round** — a builder reviewing their own work normalizes its flaws; a new critic with the rubric re-derives standards every iteration.
- **Screenshot-scored, not prose-scored** — the rubric is applied to committed per-zone PNGs, so "is it good at near zoom" is evidence, not opinion.
- **Smoothness as a measured number** — the perf probe makes "zero lag" falsifiable (p95 frame interval, dropped%), so a pretty iteration that regressed perf can't pass.
