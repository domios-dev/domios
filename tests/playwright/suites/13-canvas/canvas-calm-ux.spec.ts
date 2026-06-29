/**
 * 13-canvas — calm UX gates (L4): progressive presence disclosure, compact swarm,
 * no dead pane-plan overlay, workspace plan row as action.
 *
 *   npx playwright test --config=tests/playwright/playwright.config.ts \
 *     --project=golden -g @canvas-calm-ux --retries=0 --workers=1
 */
import { expect, test } from "../../helpers/fixtures";
import {
  CANVAS_BOARD_SEL,
  isEmpty,
  seedPerfFixtureAndOpenInlineCanvas,
  setCanvasScale,
} from "../../helpers/canvas-inspect";

async function assertCalmPresenceAtFarZoom(page: import("@playwright/test").Page): Promise<void> {
  const pinsFar = await page.locator('[data-testid="canvas-presence-pin"]').count();
  expect(pinsFar, "live worker pins at far zoom").toBeGreaterThan(0);
  const revealedFar = await page.locator('[data-testid="canvas-presence-pin"][data-presence-reveal="true"]').count();
  const linksFar = await page.locator('[data-testid="canvas-presence-link"]').count();
  expect(linksFar, "connectors only on revealed pins at far zoom").toBe(revealedFar);
  expect(pinsFar - revealedFar, "some pins stay calm at far zoom").toBeGreaterThan(0);
}

test("@golden @canvas-calm-ux calm canvas surfaces and progressive presence", async ({ api, ui }) => {
  test.setTimeout(180_000);
  const { page } = ui;

  await seedPerfFixtureAndOpenInlineCanvas(page, api);
  if (await isEmpty(page)) {
    test.skip(true, "canvas empty — perf fixture did not inject");
    return;
  }
  await expect(page.locator(CANVAS_BOARD_SEL).first()).toBeVisible({ timeout: 30_000 });
  await expect(page.locator('[data-testid="pane-plan-overlay"]')).toHaveCount(0);

  await setCanvasScale(page, 0.45);
  await page.waitForTimeout(300);
  await assertCalmPresenceAtFarZoom(page);

  const firstPin = page.locator('[data-testid="canvas-presence-overlay"]').first();
  await firstPin.hover();
  await page.waitForTimeout(120);
  const linksHover = await page.locator('[data-testid="canvas-presence-link"]').count();
  expect(linksHover, "hover reveals connector").toBeGreaterThan(0);

  await setCanvasScale(page, 2.2);
  await page.waitForTimeout(300);
  const revealed = await page.locator('[data-testid="canvas-presence-pin"][data-presence-reveal="true"]').count();
  expect(revealed, "deep zoom reveals presence details").toBeGreaterThan(0);

  const cluster = page.locator('[data-testid="canvas-swarm-cluster"]');
  if ((await cluster.count()) > 0) {
    await expect(cluster).toHaveAttribute("data-swarm-expanded", "false");
    await expect(cluster.locator('[data-testid="canvas-swarm-face"]')).toHaveCount(0);
    await cluster.locator('[data-testid="canvas-swarm-expand"]').click();
    await expect(cluster).toHaveAttribute("data-swarm-expanded", "true");
    await expect(cluster.locator('[data-testid="canvas-swarm-member"]').first()).toBeVisible();
  }
});
