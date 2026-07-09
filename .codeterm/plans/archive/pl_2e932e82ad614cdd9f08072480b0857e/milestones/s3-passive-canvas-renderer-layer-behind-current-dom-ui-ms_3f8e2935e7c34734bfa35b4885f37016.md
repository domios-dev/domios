---
plan_id: pl_2e932e82ad614cdd9f08072480b0857e
kind: milestone
id: ms_3f8e2935e7c34734bfa35b4885f37016
handle: s3-passive-canvas-renderer-layer-behind-current-dom-ui-ms_3f8e2935e7c34734bfa35b4885f37016
title: status_update
---

Accepted passive renderer lane with orchestrator fixes.

Integrated changes:
- Added feature flag `canvas.passiveRenderer`.
- Added passive Canvas2D renderer layer behind the existing DOM cards.
- Added `window.__codetermCanvasPassive` debug hooks for pixel sampling and projected box readback.
- Extracted Canvas focus breadcrumb component.
- Extended Figma acceptance helper with passive renderer flag/selector.
- Added Playwright gate `@canvas-passive-renderer`.

Orchestrator adjustments after worker report:
- Added real Playwright test because worker did not include one for the passive flag.
- Fixed the test oracle to fit the board before sampling pixels.
- Reduced passive-renderer hot-path cost: read CSS variables through computed style instead of injecting probe DOM nodes every draw; cull off-screen groups, edges, and nodes before path/text work; skip text labels below readable size.

Gates run in hot checkout:
- `npx tsx src/components/Canvas/passiveRenderer.test.ts` — 8 passed.
- `npx tsx src/components/Canvas/scene.test.ts` — 16 passed.
- `npx tsc --noEmit` — passed.
- `pnpm build` — passed.
- `npx playwright test --config=tests/playwright/playwright.config.ts --project=golden --grep "@canvas-passive-renderer|@canvas-figma-fixtures" --reporter=line --timeout=60000` — 2 passed.
- `bash scripts/check-file-sizes.sh` — reports pre-existing oversized files outside this lane; touched files are under 1000 lines.

Residual risk:
- This slice is observational/passive and still leaves DOM cards mounted, so it is not expected to solve the user-visible canvas jank by itself. The next performance slice must reduce DOM card mounts via canvas/LOD overlay budget.
