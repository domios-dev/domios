/**
 * 13-canvas — SECTION ZONES (territory-by-section).
 *
 * Tasks of the same section pack as one cohesive cluster, and each section group is
 * wrapped in a labeled ORGANIC region (translucent blob + name pill on its top edge)
 * drawn UNDER the cards. Zones are the W1/W2 territory glance and recede at W3.
 * Verified on the perf fixture (tasks spread across the rich plan sections).
 *
 *   npx playwright test --config=tests/playwright/playwright.config.ts \
 *     --project=golden -g @canvas-section-zones --retries=0 --workers=1
 */
import { expect, test } from "../../helpers/fixtures";
import type { Page } from "@playwright/test";
import { findOverflows, isEmpty, openCanvas, setCanvasScale, setCanvasScaleOn } from "../../helpers/canvas-inspect";

const PERF_KEY = "canvas.perfFixtureN";
const SUBSTRATE_KEY = "canvas.substrateV2";
const PLAN_SEL = '[data-testid="canvas-plan-node"]';

async function shot(page: Page, name: string): Promise<void> {
  await page.locator('[data-testid="canvas-view"]').first().screenshot({ path: `/tmp/${name}.png` }).catch(async () => {
    await page.screenshot({ path: `/tmp/${name}.png` });
  });
}

async function reach(page: Page, target: string, scales: number[]): Promise<string | null> {
  let band = await page.getAttribute(PLAN_SEL, "data-world").catch(() => null);
  for (const s of scales) {
    await setCanvasScaleOn(page, PLAN_SEL, s);
    await page.waitForTimeout(220);
    band = await page.getAttribute(PLAN_SEL, "data-world").catch(() => null);
    if (band === target) break;
  }
  return band;
}

test("@golden @canvas-section-zones labeled organic territories cluster tasks by section", async ({ ui }) => {
  test.setTimeout(300_000);
  const { page } = ui;

  await page.addInitScript(([perfKey, subKey]) => {
    try {
      window.localStorage.setItem(perfKey, "24");
      window.localStorage.setItem(subKey, "true");
    } catch {
      /* surfaced below */
    }
  }, [PERF_KEY, SUBSTRATE_KEY]);
  await page.reload({ waitUntil: "domcontentloaded" });
  await page.waitForSelector("#root > *", { state: "attached", timeout: 90_000 });

  await openCanvas(page);
  if (await isEmpty(page)) {
    test.skip(true, "canvas empty — perf fixture did not inject");
    return;
  }
  await expect(page.locator(PLAN_SEL).first()).toBeVisible({ timeout: 30_000 });

  await setCanvasScale(page, 0.18);
  await page.waitForTimeout(300);
  await shot(page, "zones2-W0");

  await setCanvasScale(page, 0.5);
  await page.waitForTimeout(300);
  await shot(page, "zones2-W1");
  const zoneCount = await page.locator('[data-testid="canvas-section-zone"]').count();
  expect(zoneCount, "visible section territory hulls present").toBeGreaterThan(1);
  const labelCount = await page.locator('[data-testid="canvas-section-zone-label"]').count();
  expect(labelCount, "region name pills present").toBeGreaterThan(0);
  await shot(page, "labels-W1");

  // multi-word section name renders complete.
  const labelTexts = await page.locator('[data-testid="canvas-section-zone-label"]').allTextContents();
  expect(labelTexts.some((t) => t.replace(/\s+/g, " ").includes("Worlds & content-LOD")), `a full multi-word title renders complete: ${JSON.stringify(labelTexts)}`).toBe(true);
  expect(labelTexts.every((t) => !t.includes("…")), `no zone label is ellipsis-truncated: ${JSON.stringify(labelTexts)}`).toBe(true);

  // PHYSICAL labels: a region name pill must not overlap any task card (reserved in pack).
  const labelOnCard = await page.evaluate(() => {
    const labels = [...document.querySelectorAll('[data-testid="canvas-section-zone-label"]')].map((e) => e.getBoundingClientRect());
    const cards = [...document.querySelectorAll('[data-testid="canvas-task-card"]')].map((e) => e.getBoundingClientRect());
    let hits = 0;
    for (const l of labels) for (const c of cards) {
      const x = Math.max(0, Math.min(l.right, c.right) - Math.max(l.left, c.left));
      const y = Math.max(0, Math.min(l.bottom, c.bottom) - Math.max(l.top, c.top));
      if (x * y > 120) hits++;
    }
    return hits;
  });
  expect(labelOnCard, "region labels do not overlap task cards (reserved in pack)").toBe(0);

  const w2 = await reach(page, "W2", [1.8, 2.2, 2.6, 3.0]);
  await shot(page, "zones2-W2");
  const w3 = await reach(page, "W3", [3.8, 4.6, 5.4, 6.2]);
  expect([w2, w3].includes("W2") || [w2, w3].includes("W3"), `reached a reading band (w2=${w2} w3=${w3})`).toBe(true);

  const overflows = await findOverflows(page, 2);
  expect(overflows, `zero overflow on plan/task cards: ${JSON.stringify(overflows)}`).toEqual([]);

  console.log("section-zones:", JSON.stringify({ w2, w3 }), `zones=${zoneCount} labels=${labelCount} — /tmp/zones2-*.png`);
});
