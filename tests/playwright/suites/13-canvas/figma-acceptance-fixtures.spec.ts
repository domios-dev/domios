/**
 * 13-canvas — deterministic large-world layout fixtures (100/500/1000 tasks).
 * Pure layout acceptance; no dev daemon required.
 *
 *   npx playwright test --config=tests/playwright/playwright.config.ts \
 *     --project=golden --grep @canvas-figma-fixtures
 */
import { expect, test } from "@playwright/test";
import {
  LARGE_CANVAS_WORLD_SIZES,
  largeCanvasFingerprint,
  largeCanvasLayoutStats,
} from "../../../../src/components/Canvas/layout/largeCanvasFixtures";

test("@golden @canvas-figma-fixtures large canvas worlds are deterministic with zero core overlap", async () => {
  for (const size of LARGE_CANVAS_WORLD_SIZES) {
    const a = largeCanvasFingerprint(size);
    const b = largeCanvasFingerprint(size);
    expect(a, `${size} fingerprint stable`).toBe(b);
    const stats = largeCanvasLayoutStats(size);
    expect(stats.taskCount, `${size} task nodes`).toBe(size);
    expect(stats.coreOverlapCount, `${size} no core overlap`).toBe(0);
    expect(stats.bounds.width, `${size} bounds width`).toBeGreaterThan(0);
    expect(stats.bounds.height, `${size} bounds height`).toBeGreaterThan(0);
  }
});
