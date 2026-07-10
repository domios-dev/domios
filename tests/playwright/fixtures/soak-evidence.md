# Phase 3 Soak Evidence

Date: 2026-07-08

Migrated tests:

- `13-canvas/plan-view-mode.spec.ts` — plan surface exposes board/list/canvas switch
- `13-canvas/plan-view-mode.spec.ts` — plan view mode persists across close and reopen
- `13-canvas/mission-control-flow.spec.ts` — flow river + group milestone + all-plans toggle
- `13-canvas/mission-control-flow.spec.ts` — 60fps pan probe
- `13-canvas/plan-detail-overlay.spec.ts` — board task card opens detail overlay and Esc closes it

Before/stabilization soak command:

```sh
pnpm exec playwright test -c tests/playwright --project=golden \
  tests/playwright/suites/13-canvas/plan-view-mode.spec.ts \
  tests/playwright/suites/13-canvas/mission-control-flow.spec.ts \
  tests/playwright/suites/13-canvas/plan-detail-overlay.spec.ts \
  --grep "plan surface exposes|persists across|flow river|60fps pan probe|board task card opens" \
  --repeat-each=10 --workers=1
```

Before/stabilization result: `48 passed, 2 flaky` in 3.6m. Both flaky retries
were in `mission-control-flow.spec.ts` after reload lost the inline canvas shell:

- `flow river + group milestone + all-plans toggle`: first attempt failed waiting
  for `[data-testid="plan-view-shell"]`; retry passed.
- `60fps pan probe`: same missing-shell reload path; retry passed.

Artifacts:

- `tests/playwright/artifacts/test-results/13-canvas-mission-control--77034--milestone-all-plans-toggle-golden-repeat1/trace.zip`
- `tests/playwright/artifacts/test-results/13-canvas-mission-control--4135a-—-transform-layer-no-reflow-golden-repeat2/trace.zip`

Fix applied after this run: `toCanvasMode(...)` now reopens the inline workspace
canvas after reload if the plan shell is absent.

After soak command:

```sh
pnpm exec playwright test -c tests/playwright --project=golden \
  tests/playwright/suites/13-canvas/plan-view-mode.spec.ts \
  tests/playwright/suites/13-canvas/mission-control-flow.spec.ts \
  tests/playwright/suites/13-canvas/plan-detail-overlay.spec.ts \
  --grep "plan surface exposes|persists across|flow river|60fps pan probe|board task card opens" \
  --repeat-each=10 --workers=1
```

After result: `50 passed` in 2.4m, no retries/flakes.

Frame probe range observed in the final run: median `16.6-16.7ms`, p95
`18.4-18.6ms` for the printed pan-probe samples.
