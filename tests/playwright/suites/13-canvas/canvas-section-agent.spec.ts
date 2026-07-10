/**
 * WHAT: canvas section agent covers its Playwright acceptance contract.
 * WHY: This spec protects the live CodeTerm workflow from regressions in 13-canvas.
 * ORACLE: The test asserts backend state and/or stable UI hooks instead of relying on smoke-only visibility.
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
    // PW_JUSTIFIED_SLEEP: fixed 220ms sampling/settle window; no app event exists for this probe.
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

  await indicators.first().dispatchEvent("click");
  // PW_JUSTIFIED_SLEEP: fixed 900ms sampling/settle window; no app event exists for this probe.
  await page.waitForTimeout(900);
  await shot(page, "secagent-jumped");
  await expect(page.locator('[data-testid="canvas-breadcrumb"]'), "clicking the indicator recorded a nav hop").toBeVisible();

  console.log(`section-agent: indicators=${await indicators.count()} band=${band} — /tmp/secagent-*.png`);
});
