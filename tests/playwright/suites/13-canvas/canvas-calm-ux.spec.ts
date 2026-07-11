/**
 * WHAT: canvas calm ux covers its Playwright acceptance contract.
 * WHY: This spec protects the live CodeTerm workflow from regressions in 13-canvas.
 * ORACLE: The test asserts backend state and/or stable UI hooks instead of relying on smoke-only visibility.
 */
import { expect, test } from "../../helpers/fixtures";
import {
  CANVAS_BOARD_SEL,
  isEmpty,
  seedPerfFixtureAndOpenInlineCanvas,
  setCanvasScale,
  visibleCanvasScope,
  waitForCanvasSettled,
} from "../../helpers/canvas-inspect";

async function assertCalmPresenceAtFarZoom(page: import("@playwright/test").Page): Promise<void> {
  const board = visibleCanvasScope(page);
  const pinsFar = await board.locator('[data-testid="canvas-presence-pin"]').count();
  expect(pinsFar, "live worker pins at far zoom").toBeGreaterThan(0);
}

test("@golden @canvas-calm-ux calm canvas surfaces and progressive presence", async ({ api, webBrowser }) => {
  test.setTimeout(180_000);
  const canvas = await seedPerfFixtureAndOpenInlineCanvas(webBrowser, api);
  try {
    const { page } = canvas;
    if (await isEmpty(page)) {
      test.skip(true, "canvas empty — perf fixture did not inject");
      return;
    }
    await expect(page.locator(CANVAS_BOARD_SEL).first()).toBeVisible({ timeout: 30_000 });
    await expect(page.locator('[data-testid="pane-plan-overlay"]')).toHaveCount(0);

    await setCanvasScale(page, 0.45);
    await waitForCanvasSettled(page);
    await assertCalmPresenceAtFarZoom(page);

    const firstPin = visibleCanvasScope(page).locator('[data-testid="canvas-presence-overlay"]').first();
    await firstPin.hover({ force: true, timeout: 10_000 });
    // PW_JUSTIFIED_SLEEP: fixed 120ms sampling/settle window; no app event exists for this probe.
    await page.waitForTimeout(120);
    const linksHover = await visibleCanvasScope(page).locator('[data-testid="canvas-presence-link"]').count();
    expect(linksHover, "hover reveals connector").toBeGreaterThan(0);

    await setCanvasScale(page, 2.2);
    await waitForCanvasSettled(page);
    const w2Pins = await visibleCanvasScope(page)
      .locator('[data-testid="canvas-presence-overlay"][data-presence-world="W2"], [data-testid="canvas-presence-overlay"][data-presence-world="W3"]')
      .count();
    expect(w2Pins, "deep zoom reaches W2+ presence LOD").toBeGreaterThan(0);

    const cluster = visibleCanvasScope(page).locator('[data-testid="canvas-swarm-cluster"]');
    if ((await cluster.count()) > 0) {
      await expect(cluster).toHaveAttribute("data-swarm-expanded", "false", { timeout: 5_000 });
      await expect(cluster.locator('[data-testid="canvas-swarm-roster"]')).toHaveCount(0, { timeout: 5_000 });
      await cluster.locator('[data-testid="canvas-swarm-expand"]').click({ timeout: 5_000 });
      await expect(cluster).toHaveAttribute("data-swarm-expanded", "true", { timeout: 5_000 });
      await expect(cluster.locator('[data-testid="canvas-swarm-roster"]')).toBeVisible({ timeout: 5_000 });
      await expect(cluster.locator('[data-testid="canvas-swarm-member"]').first()).toBeVisible({ timeout: 5_000 });
    }
  } finally {
    await canvas.close();
  }
});
