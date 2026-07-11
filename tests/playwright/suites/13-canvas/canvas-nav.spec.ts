/**
 * WHAT: canvas nav covers its Playwright acceptance contract.
 * WHY: This spec protects the live CodeTerm workflow from regressions in 13-canvas.
 * ORACLE: The test asserts backend state and/or stable UI hooks instead of relying on smoke-only visibility.
 */
import { expect, test } from "../../helpers/fixtures";
import type { Page } from "@playwright/test";
import { isEmpty, openCanvas, setCanvasScale,
  waitForCanvasSettled,
} from "../../helpers/canvas-inspect";

const PERF_KEY = "canvas.perfFixtureN";
const SUBSTRATE_KEY = "canvas.substrateV2";
const PLAN_SEL = '[data-testid="canvas-plan-node"]';

async function shot(page: Page, name: string): Promise<void> {
  await page.locator('[data-testid="canvas-view"]').first().screenshot({ path: `/tmp/${name}.png` }).catch(async () => {
    await page.screenshot({ path: `/tmp/${name}.png` });
  });
}

test("@golden @canvas-nav fly into a task, then Home always returns to the plan", async ({ ui }) => {
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

  // At home (the plan) there is no breadcrumb.
  await setCanvasScale(page, 0.55);
  await waitForCanvasSettled(page);
  expect(await page.locator('[data-testid="canvas-breadcrumb"]').count(), "no breadcrumb at home").toBe(0);

  // FLY: click a live worker pin → fly to its task (records a nav hop).
  const pin = page.locator('[data-testid="canvas-presence-overlay"]').first();
  await expect(pin).toBeVisible({ timeout: 15_000 });
  await pin.click();
  await waitForCanvasSettled(page, 2_500);
  await shot(page, "nav-deep");

  // The return-to-plan affordance is now present: ⌂ home crumb + ≥1 path crumb + Back.
  await expect(page.locator('[data-testid="canvas-breadcrumb"]')).toBeVisible();
  await expect(page.locator('[data-testid="canvas-breadcrumb-home"]')).toBeVisible();
  expect(await page.locator('[data-testid="canvas-breadcrumb-crumb"]').count(), "the flown path has a crumb").toBeGreaterThan(0);

  // HOME: click ⌂ → reframe the plan + clear the path (breadcrumb disappears).
  await page.locator('[data-testid="canvas-breadcrumb-home"]').click();
  await waitForCanvasSettled(page, 2_500);
  await shot(page, "nav-home");
  expect(await page.locator('[data-testid="canvas-breadcrumb"]').count(), "home cleared the path → no breadcrumb").toBe(0);
  await expect(page.locator(PLAN_SEL).first()).toBeVisible();

  console.log("nav: flew to task, home returned to plan — /tmp/nav-deep.png /tmp/nav-home.png");
});
