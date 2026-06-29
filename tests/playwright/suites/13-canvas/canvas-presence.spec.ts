/**
 * 13-canvas — WORKERS AS USERS (live presence).
 *
 * Progressive disclosure: far zoom shows dot/avatar only; connector/action/flag
 * appear on hover, deep zoom (W2+), or selected emphasis.
 *
 *   npx playwright test --config=tests/playwright/playwright.config.ts \
 *     --project=golden -g @canvas-presence --retries=0 --workers=1
 */
import { expect, test } from "../../helpers/fixtures";
import type { Page } from "@playwright/test";
import { isEmpty, openCanvas, setCanvasScale } from "../../helpers/canvas-inspect";

const PERF_KEY = "canvas.perfFixtureN";
const SUBSTRATE_KEY = "canvas.substrateV2";
const PLAN_SEL = '[data-testid="canvas-plan-node"], [data-testid="canvas-world-canvas"]';

async function shot(page: Page, name: string): Promise<void> {
  await page.locator('[data-testid="canvas-view"]').first().screenshot({ path: `/tmp/${name}.png` }).catch(async () => {
    await page.screenshot({ path: `/tmp/${name}.png` });
  });
}

test("@golden @canvas-presence live workers render with progressive presence disclosure", async ({ ui }) => {
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

  // ── W0/W1: avatars/dots without connectors. ──
  await setCanvasScale(page, 0.55);
  await page.waitForTimeout(300);
  await shot(page, "presence2-W1");

  const pins = await page.locator('[data-testid="canvas-presence-pin"]').count();
  expect(pins, "≥1 live worker pin present").toBeGreaterThan(0);
  const workers = await page.locator('[data-testid="canvas-presence-overlay"]').count();
  expect(workers, "≥1 live worker avatar/dot present").toBeGreaterThan(0);
  const links = await page.locator('[data-testid="canvas-presence-link"]').count();
  expect(links, "connectors hidden at far/mid zoom").toBe(0);
  expect(workers, "every pin has a marker").toBe(pins);
  const first = page.locator('[data-testid="canvas-presence-pin"]').first();
  expect(await first.getAttribute("data-task-id"), "worker pin is bound to a task").toBeTruthy();

  // Hover reveals connector + flag for the hovered pin.
  await page.locator('[data-testid="canvas-presence-overlay"]').first().hover();
  await page.waitForTimeout(150);
  const linksHover = await page.locator('[data-testid="canvas-presence-link"]').count();
  expect(linksHover, "hover reveals ≥1 connector").toBeGreaterThan(0);

  // ── W2: deep zoom reveals action lines on emphasized pins. ──
  await setCanvasScale(page, 2.0);
  await page.waitForTimeout(300);
  await shot(page, "presence2-W2");
  const revealed = await page.locator('[data-testid="canvas-presence-pin"][data-presence-reveal="true"]').count();
  expect(revealed, "≥1 pin at reveal LOD at W2").toBeGreaterThan(0);
  const actions = await page.locator('[data-testid="canvas-presence-action"]').count();
  expect(actions, "≥1 worker shows a current-action line at W2").toBeGreaterThan(0);

  console.log(`presence: workers=${workers} revealed=${revealed} actions=${actions} — /tmp/presence2-*.png`);
});
