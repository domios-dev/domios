/**
 * 13-canvas — PER-SECTION LIVE AGENT.
 *
 * Reading the plan, you SEE which sections are live and who's on them: each section
 * row rolls up the LIVE agents of the tasks bound to it (task→section, live-pane
 * gated) into a quiet avatar indicator. Click it → jump to that section's live work
 * (records a nav hop, so Home/Back return to the plan).
 *
 *   npx playwright test --config=tests/playwright/playwright.config.ts \
 *     --project=golden -g @canvas-section-agent --retries=0 --workers=1
 */
import { expect, test } from "../../helpers/fixtures";
import type { Page } from "@playwright/test";
import { isEmpty, openCanvas, setCanvasScaleOn } from "../../helpers/canvas-inspect";

const PERF_KEY = "canvas.perfFixtureN";
const SUBSTRATE_KEY = "canvas.substrateV2";
const PLAN_SEL = '[data-testid="canvas-plan-node"]';

async function shot(page: Page, name: string): Promise<void> {
  await page.locator('[data-testid="canvas-view"]').first().screenshot({ path: `/tmp/${name}.png` }).catch(async () => {
    await page.screenshot({ path: `/tmp/${name}.png` });
  });
}

async function reachPlanWorld(page: Page, target: string, scales: number[]): Promise<string | null> {
  let band = await page.getAttribute(PLAN_SEL, "data-world").catch(() => null);
  for (const s of scales) {
    await setCanvasScaleOn(page, PLAN_SEL, s);
    await page.waitForTimeout(220);
    band = await page.getAttribute(PLAN_SEL, "data-world").catch(() => null);
    if (band === target) break;
  }
  return band;
}

test("@golden @canvas-section-agent section rows show their live agents; click jumps to the work", async ({ ui }) => {
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

  // W1: the section-index reading band — rows visible with their live-agent indicators.
  await reachPlanWorld(page, "W1", [0.6, 0.8, 1.0, 1.2]);
  await shot(page, "secagent-W1");
  // Then a deeper band for the document view.
  const band = await reachPlanWorld(page, "W2", [1.6, 2.0, 2.4, 2.8]);
  await shot(page, "secagent-W2");

  // At least one section row shows a live-agent indicator (a live task is bound to it).
  const indicators = page.locator('[data-testid="canvas-section-agents"]');
  expect(await indicators.count(), `a section shows its live agent(s) (plan band=${band})`).toBeGreaterThan(0);
  // The indicator carries an avatar stack (same language as the swarm).
  expect(await indicators.first().locator('[data-testid="canvas-agent-stack"]').count(), "indicator uses the agent avatar stack").toBeGreaterThan(0);

  // Click it → jump to the section's live work + record a nav hop (breadcrumb appears).
  // dispatchEvent (not .click()) fires the handler regardless of the deep-zoom scroll
  // position of the header row in the plan's document column.
  await indicators.first().dispatchEvent("click");
  await page.waitForTimeout(900);
  await shot(page, "secagent-jumped");
  await expect(page.locator('[data-testid="canvas-breadcrumb"]'), "clicking the indicator recorded a nav hop").toBeVisible();

  console.log(`section-agent: indicators=${await indicators.count()} band=${band} — /tmp/secagent-*.png`);
});
