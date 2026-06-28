**HOW the build is kept honest.** Three layers verify the canvas, each catching what the others can't:

1. **Logic units** — `tsx`/happy-dom specs next to the pure functions: `worlds.test.ts` (band selection + hysteresis), `layout.test.ts` / `taskCloud.test.ts` / `sectionZones.test.ts` (deterministic packing, no-overlap), `canvasFocusPayload.test.ts` (the emit==read focus contract), `cardContent.test.ts`/`hitTest.test.ts` (render model + geometric hit-testing). These run in milliseconds and pin the camera-decoupled invariants.

2. **Visual / no-overflow** — Playwright against a running dev (`tests/playwright/suites/13-canvas`), asserting bounding boxes (no overflow, no overlap, readable per band) and capturing per-zone screenshots at far/mid/near × per view-mode × per-facet (artifacts under `tests/playwright/artifacts/canvas/`).

3. **Perf** — `wk-perf-probe.mts` drives WebKit, samples rAF intervals during idle/pan/zoom, and reports p50/p95/max/dropped%, with `perfFixture.ts` injecting a synthetic N-task plan client-side to exercise scale (up to 20k) without the daemon ingest path.

The standing rule (detailed in the child section): **every implementation task ships its own tests and the agent self-checks via Playwright before reporting**; the orchestrator re-runs the far/mid/near + per-mode + per-facet loop and an adversarial `frontend-design` critic scores it. "Zero lag" and "no overflow" are thereby falsifiable numbers, not claims.

Children: **Test Coverage Per Feature** — the per-task testing contract.
