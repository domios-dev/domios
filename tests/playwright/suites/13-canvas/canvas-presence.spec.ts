/**
 * 13-canvas — WORKERS AS USERS (live presence).
 *
 * Every live worker (agent) renders as a USER entity: a floating avatar linked by a
 * thin connector to the task it works on — the same avatar+connector language as the
 * orchestrator (which floats as a chip linked to its plans), one tier down. Presence
 * is LIVE-ONLY (a worker with a live pane), budget-capped and viewport-culled.
 *
 *   npx playwright test --config=tests/playwright/playwright.config.ts \
 *     --project=golden -g @canvas-presence --retries=0 --workers=1
 */
import { expect, test } from "../../helpers/fixtures";
import type { Page } from "@playwright/test";
import { isEmpty, openCanvas, setCanvasScale } from "../../helpers/canvas-inspect";

const PERF_KEY = "canvas.perfFixtureN";
const SUBSTRATE_KEY = "canvas.substrateV2";
const PLAN_SEL = '[data-testid="canvas-plan-node"]';

async function shot(page: Page, name: string): Promise<void> {
  await page.locator('[data-testid="canvas-view"]').first().screenshot({ path: `/tmp/${name}.png` }).catch(async () => {
    await page.screenshot({ path: `/tmp/${name}.png` });
  });
}

test("@golden @canvas-presence live workers render as avatars + connectors, alongside the orchestrator", async ({ ui }) => {
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

  // ── W1: worker avatars + connectors to tasks; orchestrator chip + its link. ──
  await setCanvasScale(page, 0.55);
  await page.waitForTimeout(300);
  await shot(page, "presence2-W1");

  // Orchestrator entity (chip + connector to its plan).
  expect(await page.locator('[data-testid="canvas-orchestrator-group"]').count(), "orchestrator avatar present").toBeGreaterThan(0);
  expect(await page.locator('[data-testid="canvas-orchestrator-link"]').count(), "orchestrator→plan connector present").toBeGreaterThan(0);

  // Live worker entities (avatar pins) + their connectors to the task they work on.
  const workers = await page.locator('[data-testid="canvas-presence-overlay"]').count();
  expect(workers, "≥1 live worker avatar present").toBeGreaterThan(0);
  const links = await page.locator('[data-testid="canvas-presence-link"]').count();
  expect(links, "≥1 worker→task connector present").toBeGreaterThan(0);
  const first = page.locator('[data-testid="canvas-presence-overlay"]').first();
  expect(await first.getAttribute("data-task-id"), "worker pin is bound to a task").toBeTruthy();

  // ── W2: the worker shows WHAT IT'S DOING (a quiet current-action line). ──
  await setCanvasScale(page, 2.0);
  await page.waitForTimeout(300);
  await shot(page, "presence2-W2");
  const actions = await page.locator('[data-testid="canvas-presence-action"]').count();
  expect(actions, "≥1 worker shows a current-action line at W2").toBeGreaterThan(0);

  console.log(`presence: workers=${workers} links=${links} actions=${actions} — /tmp/presence2-*.png`);
});
