/**
 * WHAT: figma acceptance fixtures covers its Playwright acceptance contract.
 * WHY: This spec protects the live CodeTerm workflow from regressions in 13-canvas.
 * ORACLE: The test asserts backend state and/or stable UI hooks instead of relying on smoke-only visibility.
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
