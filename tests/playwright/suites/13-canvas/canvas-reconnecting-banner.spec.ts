/**
 * 13-canvas — reconnecting-banner hysteresis (stabilization pass).
 *
 * Proves the calm-reconnect contract:
 *  1. A healthy board never flashes the reconnecting banner across sampled
 *     checks (the "steady 200s but flashing every ~500ms" regression).
 *  2. A SINGLE /api/plans failure does NOT surface the banner (hysteresis);
 *     two consecutive failures do; recovery clears it on the first success.
 *
 * Banner testids (all gated by `reconnecting && itemCount > 0`):
 *   canvas-reconnecting / mission-control-reconnecting /
 *   plan-board-reconnecting / plan-list-reconnecting.
 */
import { expect, test } from "../../helpers/fixtures";
import { isEmpty, seedPerfFixtureAndOpenInlineCanvas } from "../../helpers/canvas-inspect";
import type { Page } from "@playwright/test";

const BANNER_SEL =
  '[data-testid="canvas-reconnecting"], [data-testid="mission-control-reconnecting"], [data-testid="plan-board-reconnecting"], [data-testid="plan-list-reconnecting"]';

const PLANS_GLOB = "**/api/plans**";

async function bannerCount(page: Page): Promise<number> {
  return page.locator(BANNER_SEL).count();
}

// Force the resilient query to re-run without a real bridge event: the hook
// re-fetches on the `codeterm-web-boot-changed` window event.
async function kickRefetch(page: Page): Promise<void> {
  await page.evaluate(() => window.dispatchEvent(new Event("codeterm-web-boot-changed")));
}

test("@golden @canvas-reconnecting healthy board never flashes reconnecting", async ({
  api,
  webBrowser,
}) => {
  test.setTimeout(180_000);
  const canvas = await seedPerfFixtureAndOpenInlineCanvas(webBrowser, api);
  try {
    const { page } = canvas;
    if (await isEmpty(page)) {
      test.skip(true, "canvas empty — perf fixture did not inject");
      return;
    }
    // Sample repeatedly across ~3s while the board stays healthy; the banner
    // must be absent every time (no lifecycle-driven flashing on refetch).
    for (let i = 0; i < 12; i++) {
      await kickRefetch(page);
      await page.waitForTimeout(250);
      expect(await bannerCount(page), `sample ${i}: banner absent on healthy board`).toBe(0);
    }
  } finally {
    await canvas.close();
  }
});

test("@golden @canvas-reconnecting one failure is silent, two show, recovery clears", async ({
  api,
  webBrowser,
}) => {
  test.setTimeout(180_000);
  const canvas = await seedPerfFixtureAndOpenInlineCanvas(webBrowser, api);
  try {
    const { page } = canvas;
    if (await isEmpty(page)) {
      test.skip(true, "canvas empty — perf fixture did not inject");
      return;
    }

    let failPlans = false;
    await page.route(PLANS_GLOB, (route) => {
      if (failPlans) return route.abort("failed");
      return route.continue();
    });

    // First failure: a single stale refetch failure stays quiet (hysteresis).
    failPlans = true;
    await kickRefetch(page);
    await page.waitForTimeout(400);
    expect(await bannerCount(page), "single failure stays quiet").toBe(0);

    // Second consecutive failure arrives via the hook's own backoff retry;
    // give it enough time (backoff attempt 1 ~= 0.5-1s) then confirm banner.
    await expect
      .poll(async () => bannerCount(page), {
        message: "two consecutive failures surface the banner",
        timeout: 8_000,
      })
      .toBeGreaterThan(0);

    // Recovery: first success clears the banner.
    failPlans = false;
    await kickRefetch(page);
    await expect
      .poll(async () => bannerCount(page), {
        message: "first success clears the banner",
        timeout: 8_000,
      })
      .toBe(0);

    await page.unroute(PLANS_GLOB);
  } finally {
    await canvas.close();
  }
});
