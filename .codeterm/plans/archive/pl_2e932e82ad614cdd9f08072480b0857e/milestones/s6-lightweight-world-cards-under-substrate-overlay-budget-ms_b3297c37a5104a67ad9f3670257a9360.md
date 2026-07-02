---
plan_id: pl_2e932e82ad614cdd9f08072480b0857e
kind: milestone
id: ms_b3297c37a5104a67ad9f3670257a9360
handle: s6-lightweight-world-cards-under-substrate-overlay-budget-ms_b3297c37a5104a67ad9f3670257a9360
title: status_update
---

Accepted S6 lightweight world cards under substrate overlay budget.

Integrated changes:
- Added `LightweightWorldCard` and focused tests.
- Budgeted world layer now renders plan/task/spec nodes as `canvas-world-lightweight` under `canvas.substrateV2=true` + `canvas.overlayBudget=true` instead of full TaskCard/SectionNode chrome.
- Focused rich content remains in `CanvasRichOverlayLayer`; world layer keeps lightweight identity/status/label hooks for navigation and focus.
- Extended overlay budget helpers/tests for substrate+budget activation.
- Updated Playwright gate to assert lightweight cards, visible rich overlay, <=2 overlays, nonblank passive canvas, and zero world-layer rich bodies.

Gates run in hot checkout:
- `npx tsx src/components/Canvas/overlayBudget.test.ts` — 10 passed.
- `npx tsx src/components/Canvas/cards/LightweightWorldCard.test.tsx` — 2 passed.
- `npx tsx src/components/Canvas/CanvasView.test.tsx` — 13/13 passed.
- `npx tsc --noEmit` — passed.
- `npx playwright test --config=tests/playwright/playwright.config.ts --project=golden --grep "@canvas-passive-renderer|@canvas-figma-fixtures" --reporter=line --timeout=60000` — 2 passed.
- `pnpm build` — passed.
- `bash scripts/check-file-sizes.sh` — reports pre-existing oversized files outside this lane; touched files are under 1000 lines.

Residual risk / next slice:
- This mode is still feature-flagged. Next work should either measure pan/zoom frame budget against a large real board or decide how/when to graduate substrate+overlay budget from debug flag toward default behavior.
