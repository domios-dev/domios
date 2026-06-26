import { expect, test } from "../../helpers/fixtures";
import {
  FIGMA_SUBSTRATE_FLAG_KEY,
  OVERLAY_BUDGET_FLAG_KEY,
  PASSIVE_RENDERER_FLAG_KEY,
  sampleCanvasPixels,
} from "../../helpers/canvas-figma-acceptance";

test("@golden @canvas-passive-renderer passive canvas layer draws budgeted substrate scene", async ({ ui }) => {
  const { page } = ui;

  await page.evaluate(
    (keys) => keys.forEach((key) => window.localStorage.setItem(key, "true")),
    [PASSIVE_RENDERER_FLAG_KEY, FIGMA_SUBSTRATE_FLAG_KEY, OVERLAY_BUDGET_FLAG_KEY],
  );
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

  const lightweightCards = page.locator('[data-testid="canvas-world-lightweight"]');
  await expect(lightweightCards.first()).toBeVisible({ timeout: 30_000 });
  await expect(lightweightCards.first()).toHaveAttribute("data-world-render", "lightweight");

  const planNode = page.locator('[data-testid="canvas-world-lightweight"][data-lightweight-kind="plan"]').first();
  await expect(planNode).toBeVisible({ timeout: 30_000 });
  await planNode.dblclick({ force: true });

  const overlay = page.locator('[data-testid="canvas-rich-overlay"]').first();
  await expect(overlay).toBeVisible({ timeout: 30_000 });
  await expect(overlay).toHaveAttribute("data-overlay-kind", /plan|task|spec/);

  const budget = await page.evaluate(() => {
    const worldLayer = document.querySelector('[data-testid="canvas-world-layer"]');
    const worldRich = worldLayer?.querySelectorAll(
      '[data-testid="canvas-plan-section-body"], [data-testid="canvas-spec-section-body"], [data-testid="canvas-task-full-body"]',
    ).length ?? 0;
    const overlays = document.querySelectorAll('[data-testid="canvas-rich-overlay"]').length;
    const readback = window.__codetermCanvasScene?.get("workspace:workspace");
    return {
      overlays,
      worldRich,
      lightweight: document.querySelectorAll('[data-testid="canvas-world-lightweight"]').length,
      readbackOverlays: readback?.activeOverlays().length ?? 0,
      maxActiveOverlays: readback?.maxActiveOverlays ?? 0,
    };
  });
  expect(budget.lightweight).toBeGreaterThan(0);
  expect(budget.overlays).toBeLessThanOrEqual(2);
  expect(budget.readbackOverlays).toBeLessThanOrEqual(2);
  expect(budget.maxActiveOverlays).toBe(2);
  expect(budget.worldRich).toBe(0);
});
