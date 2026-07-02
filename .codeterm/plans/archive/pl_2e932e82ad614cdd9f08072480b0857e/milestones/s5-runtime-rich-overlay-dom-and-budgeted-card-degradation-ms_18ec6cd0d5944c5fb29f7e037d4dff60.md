---
plan_id: pl_2e932e82ad614cdd9f08072480b0857e
kind: milestone
id: ms_18ec6cd0d5944c5fb29f7e037d4dff60
handle: s5-runtime-rich-overlay-dom-and-budgeted-card-degradation-ms_18ec6cd0d5944c5fb29f7e037d4dff60
title: status_update
---

Accepted S5 runtime rich overlay DOM and budgeted card degradation.

Integrated changes:
- Added `canvas.overlayBudget` flag and overlay budget helpers/tests.
- Added `CanvasRichOverlayLayer` that mounts focused OverlayNode content at exact `screenRect` under `canvas.substrateV2`.
- Extracted world node rendering and decorations out of CanvasView to keep CanvasView under the 1000-line cap.
- Added budgeted suppression of world-layer rich bodies for plan/task/spec while keeping focused overlay rich content.
- Extended CanvasNodeCard/nodeRegistry/TaskCard/SectionNode to support `suppressRichContent`, `richOverlay`, and overlay fill-parent rendering.
- Extended Playwright `@canvas-passive-renderer` to enable passive renderer + substrate + overlay budget flags and assert nonblank canvas, visible overlay, <=2 overlays, and zero world-layer rich bodies.

Gates run in hot checkout:
- `npx tsx src/components/Canvas/overlayBudget.test.ts` — 6 passed.
- `npx tsx src/components/Canvas/richOverlays.test.ts` — 5 passed.
- `npx tsx src/components/Canvas/CanvasView.test.tsx` — 12/12 passed.
- `npx tsc --noEmit` — passed.
- `npx playwright test --config=tests/playwright/playwright.config.ts --project=golden --grep "@canvas-passive-renderer|@canvas-figma-fixtures" --reporter=line --timeout=60000` — 2 passed with all canvas flags enabled in the passive renderer spec.
- `pnpm build` — passed.
- `bash scripts/check-file-sizes.sh` — reports pre-existing oversized files outside this lane; touched files are under 1000 lines (`CanvasView.tsx` 957, new helpers/components below 100 lines).

Residual risk / next slice:
- The world layer still mounts full card chrome; only rich markdown/body content is suppressed under budget. Further jank reduction needs a label-only/skeleton world-card mode or passive-only world layer for non-focused nodes.
