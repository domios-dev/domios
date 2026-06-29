/**
 * 13-canvas — calm UX gates (L4): progressive presence disclosure, compact swarm,
 * no dead pane-plan overlay, workspace plan row as action.
 *
 *   npx playwright test --config=tests/playwright/playwright.config.ts \
 *     --project=golden -g @canvas-calm-ux --retries=0 --workers=1
 */
import { expect, test } from "../../helpers/fixtures";
import type { Page } from "@playwright/test";
import { openCanvas, setCanvasScale } from "../../helpers/canvas-inspect";

const PERF_KEY = "canvas.perfFixtureN";
const SUBSTRATE_KEY = "canvas.substrateV2";
const PLAN_SEL = '[data-testid="canvas-plan-node"], [data-testid="canvas-world-canvas"]';

async function seedPerfFixture(page: Page): Promise<void> {
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
}

test("@golden @canvas-calm-ux calm canvas surfaces and progressive presence", async ({ ui }) => {
  test.setTimeout(180_000);
  const { page } = ui;

  // Workspace plan row reads as pinned action, not a tab clone.
  const workspace = page.locator('[data-testid="workspace-item"][data-workspace-name="codeterm"]').first();
  if (await workspace.isVisible().catch(() => false)) {
    const expand = workspace.locator('button[title="Expand"]');
    if (await expand.isVisible().catch(() => false)) await expand.click();
    const entry = page.locator('[data-testid="workspace-plan-entry"]').first();
    if (await entry.isVisible().catch(() => false)) {
      await expect(entry).toHaveAttribute("data-workspace-action", "plan");
      await expect(entry).toHaveAttribute("data-pinned", "true");
    }
  }

  await seedPerfFixture(page);
  await openCanvas(page);
  if ((await page.locator('[data-testid="canvas-empty"]').count()) > 0) {
    test.skip(true, "canvas empty — perf fixture did not inject");
    return;
  }
  await expect(page.locator(PLAN_SEL).first()).toBeVisible({ timeout: 30_000 });
  await expect(page.locator('[data-testid="pane-plan-overlay"]')).toHaveCount(0);

  // Far zoom: pins exist but connectors stay hidden until hover/deep zoom.
  await setCanvasScale(page, 0.45);
  await page.waitForTimeout(300);
  const pinsFar = await page.locator('[data-testid="canvas-presence-pin"]').count();
  expect(pinsFar, "live worker pins at far zoom").toBeGreaterThan(0);
  const linksFar = await page.locator('[data-testid="canvas-presence-link"]').count();
  expect(linksFar, "no connectors at far zoom without reveal").toBe(0);

  const firstPin = page.locator('[data-testid="canvas-presence-overlay"]').first();
  await firstPin.hover();
  await page.waitForTimeout(120);
  const linksHover = await page.locator('[data-testid="canvas-presence-link"]').count();
  expect(linksHover, "hover reveals connector").toBeGreaterThan(0);

  // Deep zoom: action lines may appear on emphasized tasks.
  await setCanvasScale(page, 2.2);
  await page.waitForTimeout(300);
  const revealed = await page.locator('[data-testid="canvas-presence-pin"][data-presence-reveal="true"]').count();
  expect(revealed, "deep zoom reveals presence details").toBeGreaterThan(0);

  // Swarm roster: compact collapsed — no face stack in the pill.
  const cluster = page.locator('[data-testid="canvas-swarm-cluster"]');
  if ((await cluster.count()) > 0) {
    await expect(cluster).toHaveAttribute("data-swarm-expanded", "false");
    await expect(cluster.locator('[data-testid="canvas-swarm-face"]')).toHaveCount(0);
    await cluster.locator('[data-testid="canvas-swarm-expand"]').click();
    await expect(cluster).toHaveAttribute("data-swarm-expanded", "true");
    await expect(cluster.locator('[data-testid="canvas-swarm-member"]').first()).toBeVisible();
  }
});
