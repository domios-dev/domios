/**
 * WHAT: plan detail overlay covers its Playwright acceptance contract.
 * WHY: This spec protects the live CodeTerm workflow from regressions in 13-canvas.
 * ORACLE: The test asserts backend state and/or stable UI hooks instead of relying on smoke-only visibility.
 */
import path from "node:path";
import { expect, test } from "../../helpers/fixtures";
import {
  assertPlanSurfaceNotEmpty,
  seedPerfFixtureAndOpenInlineCanvas,
  CANVAS_ART_DIR,
} from "../../helpers/canvas-inspect";
import { seedWorldStateAndOpenInlineCanvas, worldStates } from "../../fixtures/world-states";
import type { ApiClient } from "../../helpers/api-client";

async function screenshot(page: import("@playwright/test").Page, name: string): Promise<string> {
  const file = path.join(CANVAS_ART_DIR, `${name}.png`);
  await page.screenshot({ path: file, fullPage: false });
  return file;
}


test(
  "@golden @plan-detail-overlay board task card opens detail overlay and Esc closes it",
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

      const shell = page.locator('[data-testid="plan-view-shell"]');
      const modeSwitch = page.locator('[data-testid="plan-view-mode-switch"]');
      await expect(shell).toBeVisible({ timeout: 30_000 });
      await modeSwitch.getByRole("tab", { name: /board/i }).click();
      await expect(shell).toHaveAttribute("data-plan-view-mode", "board");
      await expect(page.locator('[data-testid="plan-board-view"]')).toBeVisible();

      const firstCard = page.locator('[data-testid="plan-board-task-card"]').first();
      await firstCard.waitFor({ state: "visible", timeout: 20_000 });

      await screenshot(page, "plan-view-board-before-detail");

      await firstCard.click();

      const overlay = page.locator('[data-testid="plan-detail-overlay"]');
      await expect(overlay).toBeVisible({ timeout: 10_000 });
      await expect(page.locator('[data-testid="plan-detail-panel"]')).toBeVisible();
      await expect(page.locator('[data-testid="plan-detail-kind"]')).toHaveText(/task/i);

      await screenshot(page, "plan-view-board-detail-task");

      await page.keyboard.press("Escape");
      await expect(overlay).toBeHidden({ timeout: 5_000 });
      await expect(page.locator('[data-testid="plan-board-view"]')).toBeVisible();
    } finally {
      await canvas.close();
    }
  },
);


test(
  "@golden @plan-detail-overlay list row opens detail overlay and Esc closes it",
  async ({ api, webBrowser }) => {
    test.setTimeout(120_000);
    const canvas = await seedPerfFixtureAndOpenInlineCanvas(webBrowser, api as unknown as ApiClient);
    try {
      const { page } = canvas;
      await assertPlanSurfaceNotEmpty(page);

      const shell = page.locator('[data-testid="plan-view-shell"]');
      const modeSwitch = page.locator('[data-testid="plan-view-mode-switch"]');
      await expect(shell).toBeVisible({ timeout: 30_000 });
      await modeSwitch.getByRole("tab", { name: /list/i }).click();
      await expect(shell).toHaveAttribute("data-plan-view-mode", "list");

      const listView = page.locator('[data-testid="plan-list-view"]');
      await expect(listView).toBeVisible();

      const taskRow = page.locator('[data-testid="plan-list-task"]').first();
      await taskRow.waitFor({ state: "visible", timeout: 20_000 });

      const scrollContainer = page.locator('[data-testid="plan-list-scroll"]');
      const scrollTopBefore = await scrollContainer.evaluate((el) => el.scrollTop).catch(() => 0);

      await screenshot(page, "plan-view-list-before-detail");

      await taskRow.click();

      const overlay = page.locator('[data-testid="plan-detail-overlay"]');
      await expect(overlay).toBeVisible({ timeout: 10_000 });
      await expect(page.locator('[data-testid="plan-detail-kind"]')).toHaveText(/task/i);

      await screenshot(page, "plan-view-list-detail-task");

      await page.keyboard.press("Escape");
      await expect(overlay).toBeHidden({ timeout: 5_000 });
      await expect(listView).toBeVisible();

      // Scroll position should be preserved (within a small tolerance)
      const scrollTopAfter = await scrollContainer.evaluate((el) => el.scrollTop).catch(() => 0);
      expect(Math.abs(scrollTopAfter - scrollTopBefore)).toBeLessThan(20);
    } finally {
      await canvas.close();
    }
  },
);


test(
  "@golden @plan-detail-overlay canvas card opens detail overlay and Esc closes it",
  async ({ api, webBrowser }) => {
    test.setTimeout(120_000);
    const canvas = await seedPerfFixtureAndOpenInlineCanvas(webBrowser, api as unknown as ApiClient);
    try {
      const { page } = canvas;
      await assertPlanSurfaceNotEmpty(page);

      const shell = page.locator('[data-testid="plan-view-shell"]');
      const modeSwitch = page.locator('[data-testid="plan-view-mode-switch"]');
      await expect(shell).toBeVisible({ timeout: 30_000 });
      await modeSwitch.getByRole("tab", { name: /canvas/i }).click();
      await expect(shell).toHaveAttribute("data-plan-view-mode", "canvas");

      const missionControl = page.locator('[data-testid="mission-control-live"]');
      await expect(missionControl).toBeVisible({ timeout: 15_000 });

      // Mission control renders flow chips (PlanRiver ChipView), not the
      // `task-<id>` TaskCard, which only the prototype gallery mounts.
      const taskCard = missionControl.locator('[data-testid^="flow-chip-"]').first();
      await expect(taskCard).toBeVisible({ timeout: 15_000 });

      await screenshot(page, "plan-view-canvas-before-detail");
      await taskCard.click();

      const overlay = page.locator('[data-testid="plan-detail-overlay"]');
      await expect(overlay).toBeVisible({ timeout: 10_000 });
      await screenshot(page, "plan-view-canvas-detail");

      await page.keyboard.press("Escape");
      await expect(overlay).toBeHidden({ timeout: 5_000 });
      await expect(missionControl).toBeVisible();
    } finally {
      await canvas.close();
    }
  },
);
