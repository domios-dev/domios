import { expect, test } from "../../helpers/fixtures";
import {
  PASSIVE_RENDERER_FLAG_KEY,
  sampleCanvasPixels,
} from "../../helpers/canvas-figma-acceptance";

test("@golden @canvas-passive-renderer passive canvas layer draws workspace scene", async ({ ui }) => {
  const { page } = ui;

  await page.evaluate((key) => window.localStorage.setItem(key, "true"), PASSIVE_RENDERER_FLAG_KEY);
  await page.reload({ waitUntil: "domcontentloaded" });

  const workspace = page.locator('[data-testid="workspace-item"][data-workspace-name="codeterm"]').first();
  await expect(workspace).toBeVisible({ timeout: 30_000 });
  const expand = workspace.locator('button[title="Expand"]');
  if (await expand.isVisible().catch(() => false)) {
    await expand.click();
  }

  const entry = page.locator('[data-testid="workspace-plan-entry"]').first();
  await expect(entry).toBeVisible({ timeout: 30_000 });
  await entry.click();

  await expect(page.locator('[data-testid="canvas-view"]').first()).toBeVisible({ timeout: 30_000 });
  await expect(page.locator('[data-testid="canvas-render-layer"]').first()).toBeVisible({ timeout: 30_000 });
  await page.getByTitle("Fit to view").click();
  await expect.poll(async () => (await sampleCanvasPixels(page)).nonblank, {
    message: "passive canvas layer draws visible pixels after fit",
    timeout: 10_000,
  }).toBe(true);

  const debug = await page.evaluate(() => {
    const registry = (window as typeof window & {
      __codetermCanvasPassive?: {
        samplePixels: () => { nonblank: boolean };
        projectedBoxes: () => unknown[];
      };
    }).__codetermCanvasPassive;
    return {
      pixels: registry?.samplePixels(),
      boxCount: registry?.projectedBoxes().length ?? 0,
    };
  });
  expect(debug.pixels?.nonblank).toBe(true);
  expect(debug.boxCount).toBeGreaterThan(0);
});
