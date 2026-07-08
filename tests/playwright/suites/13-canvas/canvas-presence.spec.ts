/**
 * WHAT: canvas presence covers its Playwright acceptance contract.
 * WHY: This spec protects the live CodeTerm workflow from regressions in 13-canvas.
 * ORACLE: The test asserts backend state and/or stable UI hooks instead of relying on smoke-only visibility.
 */
import { expect, test } from "../../helpers/fixtures";
import type { Page } from "@playwright/test";
import {
  CANVAS_BOARD_SEL,
  isEmpty,
  seedPerfFixtureAndOpenInlineCanvas,
  setCanvasScale,
  visibleCanvasScope,
} from "../../helpers/canvas-inspect";

async function shot(page: Page, name: string): Promise<void> {
  await page.locator('[data-testid="canvas-view"]').first().screenshot({ path: `/tmp/${name}.png` }).catch(async () => {
    await page.screenshot({ path: `/tmp/${name}.png` });
  });
}

test("@golden @canvas-presence live workers render with progressive presence disclosure", async ({ api, webBrowser }) => {
  test.setTimeout(300_000);
  const canvas = await seedPerfFixtureAndOpenInlineCanvas(webBrowser, api);
  try {
    const { page } = canvas;
    if (await isEmpty(page)) {
      test.skip(true, "canvas empty — perf fixture did not inject");
      return;
    }
    await expect(page.locator(CANVAS_BOARD_SEL).first()).toBeVisible({ timeout: 30_000 });

    await setCanvasScale(page, 0.55);
    // PW_JUSTIFIED_SLEEP: fixed 300ms sampling/settle window; no app event exists for this probe.
    await page.waitForTimeout(300);
    await shot(page, "presence2-W1");

    const board = visibleCanvasScope(page);
    const pins = await board.locator('[data-testid="canvas-presence-pin"]').count();
    expect(pins, "≥1 live worker pin present").toBeGreaterThan(0);
    const workers = await board.locator('[data-testid="canvas-presence-overlay"]').count();
    expect(workers, "≥1 live worker avatar/dot present").toBeGreaterThan(0);
    const linksBeforeHover = await board.locator('[data-testid="canvas-presence-link"]').count();
    expect(workers, "every pin has a marker").toBe(pins);
    const first = board.locator('[data-testid="canvas-presence-pin"]').first();
    expect(await first.getAttribute("data-task-id"), "worker pin is bound to a task").toBeTruthy();

    await board.locator('[data-testid="canvas-presence-overlay"]').first().hover();
    // PW_JUSTIFIED_SLEEP: fixed 150ms sampling/settle window; no app event exists for this probe.
    await page.waitForTimeout(150);
    const flagsHover = await board.locator('[data-testid="canvas-presence-flag"]').count();
    expect(flagsHover, "hover reveals presence label").toBeGreaterThan(0);
    const linksHover = await board.locator('[data-testid="canvas-presence-link"]').count();
    expect(linksHover, "hover keeps or reveals connector").toBeGreaterThanOrEqual(linksBeforeHover);

    await setCanvasScale(page, 2.0);
    // PW_JUSTIFIED_SLEEP: fixed 300ms sampling/settle window; no app event exists for this probe.
    await page.waitForTimeout(300);
    await shot(page, "presence2-W2");
    const w2Worlds = await board
      .locator('[data-testid="canvas-presence-overlay"]')
      .evaluateAll((els) => els.map((el) => el.getAttribute("data-presence-world")));
    expect(
      w2Worlds.some((w) => w === "W2" || w === "W3"),
      "≥1 pin at W2+ LOD at deep zoom",
    ).toBe(true);
  } finally {
    await canvas.close();
  }
});
