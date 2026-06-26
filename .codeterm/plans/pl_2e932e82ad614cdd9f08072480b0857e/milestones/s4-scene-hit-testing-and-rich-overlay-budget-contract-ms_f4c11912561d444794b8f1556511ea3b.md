Accepted S4 scene hit-testing and rich overlay budget contract.

Integrated changes:
- Added inverse-camera scene picking (`unprojectScreenPoint`, world/screen hit-test helpers) for plan/task/spec nodes.
- Added feature flag `canvas.substrateV2` for scene-geometry double-click focus fallback when DOM target has no node id.
- Added `richOverlays.ts` contract: max active overlays = 2, exact projected screen rect, markdown/Mermaid/copy capabilities, preservesWorldBox=true.
- Extended CanvasScene readback with `pickNodeAt`, `activeOverlays`, and `maxActiveOverlays` debug/acceptance hooks.
- Added/updated focused tests for scene picking, overlay budget, and CanvasView substrate double-click behavior.

Gates run in hot checkout:
- `npx tsx src/components/Canvas/scene.test.ts` — 19 passed.
- `npx tsx src/components/Canvas/richOverlays.test.ts` — 5 passed.
- `npx tsx src/components/Canvas/CanvasView.test.tsx` — 10/10 passed.
- `npx tsc --noEmit` — passed.
- `npx playwright test --config=tests/playwright/playwright.config.ts --project=golden --grep "@canvas-passive-renderer|@canvas-figma-fixtures" --reporter=line --timeout=60000` — 2 passed.
- `pnpm build` — passed.
- `bash scripts/check-file-sizes.sh` — reports pre-existing oversized files outside this lane; touched files are under 1000 lines (`CanvasView.tsx` 998 after orchestrator trim, `scene.ts` 326, `richOverlays.ts` 72, `substrateFlag.ts` 21).

Residual risk / next slice:
- Runtime rich overlay replacement is still contractual/debug-readback only. The next slice must mount actual OverlayNode DOM and then remove/cap non-focused rich DOM cards to deliver the real performance win.
