/**
 * WHAT: plan view mode covers its Playwright acceptance contract.
 * WHY: This spec protects the live CodeTerm workflow from regressions in 13-canvas.
 * ORACLE: The test asserts backend state and/or stable UI hooks instead of relying on smoke-only visibility.
 */
import path from "node:path";
import { expect, test } from "../../helpers/fixtures";
import {
  assertPlanSurfaceNotEmpty,
  MISSION_CONTROL_LIVE_SEL,
  MISSION_CONTROL_PLAN_SURFACE_SEL,
  CANVAS_ART_DIR,
} from "../../helpers/canvas-inspect";
import { seedWorldStateAndOpenInlineCanvas, worldStates } from "../../fixtures/world-states";
import type { ApiClient } from "../../helpers/api-client";


async function clearMode(page: import("@playwright/test").Page): Promise<void> {
  await page.evaluate(() => window.localStorage.removeItem("codeterm.planViewMode"));
}

async function screenshot(page: import("@playwright/test").Page, name: string): Promise<string> {
  const file = path.join(CANVAS_ART_DIR, `${name}.png`);
  await page.screenshot({ path: file, fullPage: false });
  return file;
}


test(
  "@golden @plan-view-mode plan surface exposes board list canvas switch",
  async ({ api, webBrowser }) => {
    test.setTimeout(120_000);
    const canvas = await seedWorldStateAndOpenInlineCanvas(
      webBrowser,
      api as unknown as ApiClient,
      worldStates.planWithKnownTasks,
    );
    try {
      const { page } = canvas;
      await assertPlanSurfaceNotEmpty(page);

      await clearMode(page);
      await page.reload({ waitUntil: "domcontentloaded" });
      await page.waitForSelector('[data-testid="plan-view-shell"]', { state: "visible", timeout: 30_000 });

      const shell = page.locator('[data-testid="plan-view-shell"]');
      await expect(shell).toHaveAttribute("data-plan-view-mode", "board");

      const modeSwitch = page.locator('[data-testid="plan-view-mode-switch"]');
      await expect(modeSwitch).toBeVisible();

      const tabs = modeSwitch.getByRole("tab");
      await expect(tabs.nth(0)).toHaveText(/board/i);
      await expect(tabs.nth(1)).toHaveText(/list/i);
      await expect(tabs.nth(2)).toHaveText(/canvas/i);

      await expect(modeSwitch.getByRole("tab", { name: /board/i, selected: true })).toBeVisible();
      await expect(page.locator('[data-testid="plan-board-view"]')).toBeVisible();

      await screenshot(page, "plan-view-board-default");

      await modeSwitch.getByRole("tab", { name: /list/i }).click();
      await expect(shell).toHaveAttribute("data-plan-view-mode", "list");
      await expect(page.locator('[data-testid="plan-list-view"]')).toBeVisible();
      await expect(page.locator('[data-testid="plan-board-view"]')).toBeHidden();

      await screenshot(page, "plan-view-list");

      await modeSwitch.getByRole("tab", { name: /canvas/i }).click();
      await expect(shell).toHaveAttribute("data-plan-view-mode", "canvas");
      await expect(page.locator(MISSION_CONTROL_PLAN_SURFACE_SEL)).toBeVisible();
      await expect(page.locator(MISSION_CONTROL_LIVE_SEL)).toBeVisible();
      // NOT COVERED: "canvas mode does not mount the legacy WebGL canvas view".
      // `canvas-view` lives only in CanvasViewInner, which no longer has a mount
      await expect(page.locator('[data-testid="plan-board-view"]')).toBeHidden();
      await expect(page.locator('[data-testid="plan-list-view"]')).toBeHidden();

      await screenshot(page, "plan-view-canvas");
    } finally {
      await canvas.close();
    }
  },
);


test(
  "@golden @plan-view-mode plan view mode persists across close and reopen",
  async ({ api, webBrowser }) => {
    test.setTimeout(180_000);
    const canvas = await seedWorldStateAndOpenInlineCanvas(
      webBrowser,
      api as unknown as ApiClient,
      worldStates.planWithKnownTasks,
    );
    try {
      const { page } = canvas;
      await assertPlanSurfaceNotEmpty(page);

      await clearMode(page);
      await page.reload({ waitUntil: "domcontentloaded" });
      await page.waitForSelector('[data-testid="plan-view-shell"]', { state: "visible", timeout: 30_000 });

      const shell = page.locator('[data-testid="plan-view-shell"]');
      const modeSwitch = page.locator('[data-testid="plan-view-mode-switch"]');

      await modeSwitch.getByRole("tab", { name: /list/i }).click();
      await expect(shell).toHaveAttribute("data-plan-view-mode", "list");

      await page.reload({ waitUntil: "domcontentloaded" });
      await page.waitForSelector('[data-testid="plan-view-shell"]', { state: "visible", timeout: 30_000 });
      await expect(shell).toHaveAttribute("data-plan-view-mode", "list", { timeout: 15_000 });

      await page.locator('[data-testid="plan-view-mode-switch"]').getByRole("tab", { name: /canvas/i }).click();
      await expect(shell).toHaveAttribute("data-plan-view-mode", "canvas");

      await page.reload({ waitUntil: "domcontentloaded" });
      await page.waitForSelector('[data-testid="plan-view-shell"]', { state: "visible", timeout: 30_000 });
      await expect(shell).toHaveAttribute("data-plan-view-mode", "canvas", { timeout: 15_000 });
      await expect(page.locator(MISSION_CONTROL_LIVE_SEL)).toBeVisible({ timeout: 15_000 });
    } finally {
      await canvas.close();
    }
  },
);
