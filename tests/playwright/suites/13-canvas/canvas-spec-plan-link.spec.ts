/**
 * WHAT: canvas spec plan link covers its Playwright acceptance contract.
 * WHY: This spec protects the live CodeTerm workflow from regressions in 13-canvas.
 * ORACLE: The test asserts backend state and/or stable UI hooks instead of relying on smoke-only visibility.
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

async function edgeKindCount(page: Page, kind: string): Promise<number> {
  return page.locator(`g[data-edge-kind="${kind}"]`).count();
}

test("@golden @canvas-spec-plan-link one calm spec→plan link, no per-section dot fan", async ({ ui }) => {
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

  // between them) fully in frame. The spec sits triptych-left of the plan.
  await setCanvasScale(page, 0.28);
  // PW_JUSTIFIED_SLEEP: fixed 300ms sampling/settle window; no app event exists for this probe.
  await page.waitForTimeout(300);
  await shot(page, "spec-plan-wide");
  await setCanvasScale(page, 0.5);
  // PW_JUSTIFIED_SLEEP: fixed 300ms sampling/settle window; no app event exists for this probe.
  await page.waitForTimeout(300);
  await shot(page, "spec-plan-W1");

  // EXACTLY ONE spec→plan connector; the old per-section `spec-row` fan is gone.
  expect(await edgeKindCount(page, "spec-plan"), "exactly one spec→plan link").toBe(1);
  expect(await edgeKindCount(page, "spec-row"), "no legacy per-section spec-row edges").toBe(0);

  // The connector carries NO end-dots (the user's "weird dots pointing nowhere").
  const dots = await page.evaluate(() => {
    const g = document.querySelector('g[data-edge-kind="spec-plan"]');
    return g ? g.querySelectorAll("circle").length : -1;
  });
  expect(dots, "spec→plan link has no end dots").toBe(0);

  const reach = async (target: string, scales: number[]) => {
    let band = await page.getAttribute(PLAN_SEL, "data-world").catch(() => null);
    for (const s of scales) {
      await setCanvasScaleOn(page, PLAN_SEL, s);
      // PW_JUSTIFIED_SLEEP: fixed 220ms sampling/settle window; no app event exists for this probe.
      await page.waitForTimeout(220);
      band = await page.getAttribute(PLAN_SEL, "data-world").catch(() => null);
      if (band === target) break;
    }
    return band;
  };
  const w2 = await reach("W2", [1.8, 2.2, 2.6, 3.0]);
  await shot(page, "spec-plan-W2");
  const w3 = await reach("W3", [3.8, 4.6, 5.4, 6.2]);
  await shot(page, "spec-plan-W3");
  expect([w2, w3].includes("W2") || [w2, w3].includes("W3"), `reached a reading band (w2=${w2} w3=${w3})`).toBe(true);

  // Inline per-section provenance chips still present (the correct linkage UI).
  const bubbles = page.locator('[data-testid="canvas-plan-spec-ref"]');
  expect(await bubbles.count(), "inline 'From spec' chips render per section").toBeGreaterThan(0);

  // Connector count is render-stable across bands (still exactly one).
  expect(await edgeKindCount(page, "spec-plan"), "still exactly one link at reading band").toBe(1);

  const overflows = await findOverflows(page, 2);
  expect(overflows, `zero overflow on plan/task cards: ${JSON.stringify(overflows)}`).toEqual([]);

  console.log("spec-plan bands:", JSON.stringify({ w2, w3 }), "— screenshots in /tmp/spec-plan-*.png");
});
