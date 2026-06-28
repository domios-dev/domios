/**
 * 13-canvas — VISUAL SWEEP (orchestrator looking-glass, NOT an assertion gate).
 *
 * Drives the real browser through every zoom band + the task-cloud region and dumps
 * screenshots to /tmp/sweep-*.png so a human (or the orchestrator) can LOOK at how
 * the canvas actually renders — broken labels, zone shapes, readability at distance —
 * instead of trusting algorithmic asserts. Run it, then open the PNGs.
 *
 *   CT_E2E_PORT=7686 npx playwright test --config=tests/playwright/playwright.config.ts \
 *     --project=golden -g @canvas-visual-sweep --retries=0 --workers=1
 *
 * Override the fixture: CANVAS_SWEEP_PERF=24 (default) sets canvas.perfFixtureN.
 */
import { test } from "../../helpers/fixtures";
import type { Page } from "@playwright/test";
import fs from "node:fs";
import path from "node:path";
import { canvasScale, isEmpty, openCanvas, panToMountTaskCards, setCanvasScaleOn } from "../../helpers/canvas-inspect";

const PERF_KEY = "canvas.perfFixtureN";
const SUBSTRATE_KEY = "canvas.substrateV2";
const PLAN_SEL = '[data-testid="canvas-plan-node"]';
const PERF_N = process.env.CANVAS_SWEEP_PERF || "24";

const BANDS: Array<{ name: string; scale: number }> = [
  { name: "W0-far", scale: 0.45 },
  { name: "W1-glance", scale: 1.0 },
  { name: "W2-read", scale: 2.3 },
  { name: "W3-deep", scale: 4.6 },
];

async function shot(page: Page, name: string): Promise<void> {
  await page
    .locator('[data-testid="canvas-view"]')
    .first()
    .screenshot({ path: `/tmp/${name}.png` })
    .catch(async () => {
      await page.screenshot({ path: `/tmp/${name}.png` });
    });
}

test("@golden @canvas-visual-sweep capture every band + task cloud for visual review", async ({ ui }) => {
  test.setTimeout(300_000);
  const { page } = ui;

  // REAL mode (CANVAS_SWEEP_REAL=1): inject the ACTUAL dogfood plan JSON from disk
  // via canvas.realPlan — the faithful gate on what the user really sees, and the
  // path that carries the real owner/orchestrator. Default: synthetic perf fixture.
  const real = process.env.CANVAS_SWEEP_REAL === "1";
  const planPath =
    process.env.CANVAS_SWEEP_PLAN ||
    path.resolve(__dirname, "..", "..", "..", "..", ".codeterm", "plans", "pl_c8a72a9a870743ab81fb8ac94e0e459f.json");
  const realPlanJson = real ? fs.readFileSync(planPath, "utf8") : "";
  if (real) console.log(`SWEEP REAL: injecting plan ${planPath} (${realPlanJson.length} bytes)`);
  await page.addInitScript(
    ([perfKey, subKey, perfN, realMode, planJson]) => {
      try {
        if (realMode === "1") {
          window.localStorage.removeItem(perfKey);
          window.localStorage.setItem("canvas.realPlan", "1");
          window.localStorage.setItem("canvas.realPlanJson", planJson);
        } else {
          window.localStorage.setItem(perfKey, perfN);
        }
        window.localStorage.setItem(subKey, "true");
      } catch {
        /* ignore */
      }
    },
    [PERF_KEY, SUBSTRATE_KEY, PERF_N, real ? "1" : "0", realPlanJson],
  );
  // addInitScript only takes effect on a navigation — reload so the fixture injects
  // (without this the canvas mounts empty unless localStorage leaked from a prior run).
  await page.reload({ waitUntil: "domcontentloaded" });
  await page.waitForSelector("#root > *", { state: "attached", timeout: 90_000 });

  await openCanvas(page);
  if (await isEmpty(page)) {
    console.log("SWEEP: canvas empty — nothing to capture");
    return;
  }

  // 1) Whole-plan triptych at each band, anchored on the plan so it stays framed.
  for (const band of BANDS) {
    await setCanvasScaleOn(page, PLAN_SEL, band.scale);
    const actual = await canvasScale(page);
    await shot(page, `sweep-plan-${band.name}`);
    console.log(`SWEEP plan ${band.name}: target=${band.scale} actual=${actual.toFixed(2)} -> /tmp/sweep-plan-${band.name}.png`);
  }

  // 1b) Overview / orchestrator avatar — fit the whole board, capture the floating
  // orchestrator chip + its connector (only present when owner groups exist).
  await page.locator('[data-testid="canvas-preset-overview"]').click().catch(() => {});
  await page.waitForTimeout(500);
  await shot(page, "sweep-overview");
  const orch = await page.locator('[data-testid="canvas-orchestrator-group"]').count();
  console.log(`SWEEP: orchestrator-group count=${orch} -> /tmp/sweep-overview.png`);

  // 2) Task cloud (where section zones + clustered cards live) at glance + read.
  const cards = await panToMountTaskCards(page, 1.4);
  console.log(`SWEEP: mounted ${cards.length} task cards in cloud`);
  if (cards.length > 0) {
    await shot(page, "sweep-cloud-W1-glance");
    await setCanvasScaleOn(page, '[data-testid="canvas-task-card"]', 2.4);
    await shot(page, "sweep-cloud-W2-read");
    console.log("SWEEP cloud -> /tmp/sweep-cloud-W1-glance.png /tmp/sweep-cloud-W2-read.png");
  }
});
