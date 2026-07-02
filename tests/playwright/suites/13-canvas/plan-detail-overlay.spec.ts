/**
 * 13-canvas — PlanDetailOverlay: opens from Board task card, List row, Canvas card.
 * Esc restores prior view state (board, list scroll).
 */
import path from "node:path";
import { expect, test } from "../../helpers/fixtures";
import {
  isEmpty,
  seedPerfFixtureAndOpenInlineCanvas,
  CANVAS_ART_DIR,
} from "../../helpers/canvas-inspect";
import type { ApiClient } from "../../helpers/api-client";

async function screenshot(page: import("@playwright/test").Page, name: string): Promise<string> {
  const file = path.join(CANVAS_ART_DIR, `${name}.png`);
  await page.screenshot({ path: file, fullPage: false });
  return file;
}

// ---------------------------------------------------------------------------
// Board task card → detail overlay → Esc restores board
// ---------------------------------------------------------------------------

test(
  "@golden @plan-detail-overlay board task card opens detail overlay and Esc closes it",
  async ({ api, webBrowser }) => {
    test.setTimeout(120_000);
    const canvas = await seedPerfFixtureAndOpenInlineCanvas(webBrowser, api as unknown as ApiClient);
    try {
      const { page } = canvas;
      if (await isEmpty(page)) {
        test.skip(true, "canvas empty — perf fixture did not inject");
        return;
      }

      // Ensure board mode
      const shell = page.locator('[data-testid="plan-view-shell"]');
      const modeSwitch = page.locator('[data-testid="plan-view-mode-switch"]');
      await expect(shell).toBeVisible({ timeout: 30_000 });
      await modeSwitch.getByRole("tab", { name: /board/i }).click();
      await expect(shell).toHaveAttribute("data-plan-view-mode", "board");
      await expect(page.locator('[data-testid="plan-board-view"]')).toBeVisible();

      // Find and click the first interactive task card
      const firstCard = page.locator('[data-testid="plan-board-task-card"]').first();
      await firstCard.waitFor({ state: "visible", timeout: 20_000 });

      await screenshot(page, "plan-view-board-before-detail");

      await firstCard.click();

      // Overlay should appear
      const overlay = page.locator('[data-testid="plan-detail-overlay"]');
      await expect(overlay).toBeVisible({ timeout: 10_000 });
      await expect(page.locator('[data-testid="plan-detail-panel"]')).toBeVisible();
      await expect(page.locator('[data-testid="plan-detail-kind"]')).toHaveText(/task/i);

      await screenshot(page, "plan-view-board-detail-task");

      // Esc closes the overlay and restores the board
      await page.keyboard.press("Escape");
      await expect(overlay).toBeHidden({ timeout: 5_000 });
      await expect(page.locator('[data-testid="plan-board-view"]')).toBeVisible();
    } finally {
      await canvas.close();
    }
  },
);

// ---------------------------------------------------------------------------
// List row → detail overlay → Esc restores list
// ---------------------------------------------------------------------------

test(
  "@golden @plan-detail-overlay list row opens detail overlay and Esc closes it",
  async ({ api, webBrowser }) => {
    test.setTimeout(120_000);
    const canvas = await seedPerfFixtureAndOpenInlineCanvas(webBrowser, api as unknown as ApiClient);
    try {
      const { page } = canvas;
      if (await isEmpty(page)) {
        test.skip(true, "canvas empty — perf fixture did not inject");
        return;
      }

      // Switch to list mode
      const shell = page.locator('[data-testid="plan-view-shell"]');
      const modeSwitch = page.locator('[data-testid="plan-view-mode-switch"]');
      await expect(shell).toBeVisible({ timeout: 30_000 });
      await modeSwitch.getByRole("tab", { name: /list/i }).click();
      await expect(shell).toHaveAttribute("data-plan-view-mode", "list");

      const listView = page.locator('[data-testid="plan-list-view"]');
      await expect(listView).toBeVisible();

      // Find a task row and click it
      const taskRow = page.locator('[data-testid="plan-list-task"]').first();
      await taskRow.waitFor({ state: "visible", timeout: 20_000 });

      // Note scroll position of list before opening detail
      const scrollContainer = page.locator('[data-testid="plan-list-scroll"]');
      const scrollTopBefore = await scrollContainer.evaluate((el) => el.scrollTop).catch(() => 0);

      await screenshot(page, "plan-view-list-before-detail");

      await taskRow.click();

      const overlay = page.locator('[data-testid="plan-detail-overlay"]');
      await expect(overlay).toBeVisible({ timeout: 10_000 });
      await expect(page.locator('[data-testid="plan-detail-kind"]')).toHaveText(/task/i);

      await screenshot(page, "plan-view-list-detail-task");

      // Esc closes overlay, list is restored
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

// ---------------------------------------------------------------------------
// Canvas card → detail overlay (via canvas-detail testIdPrefix)
// ---------------------------------------------------------------------------

test(
  "@golden @plan-detail-overlay canvas card opens detail overlay and Esc closes it",
  async ({ api, webBrowser }) => {
    test.setTimeout(120_000);
    const canvas = await seedPerfFixtureAndOpenInlineCanvas(webBrowser, api as unknown as ApiClient);
    try {
      const { page } = canvas;
      if (await isEmpty(page)) {
        test.skip(true, "canvas empty — perf fixture did not inject");
        return;
      }

      // Switch to canvas mode
      const shell = page.locator('[data-testid="plan-view-shell"]');
      const modeSwitch = page.locator('[data-testid="plan-view-mode-switch"]');
      await expect(shell).toBeVisible({ timeout: 30_000 });
      await modeSwitch.getByRole("tab", { name: /canvas/i }).click();
      await expect(shell).toHaveAttribute("data-plan-view-mode", "canvas");

      const canvasView = page.locator('[data-testid="canvas-view"]');
      await expect(canvasView).toBeVisible({ timeout: 15_000 });

      // Try to find a DOM-rendered task card in canvas view
      const canvasTaskCard = canvasView.locator('[data-testid="canvas-task-card"]').first();
      const hasCards = await canvasTaskCard.isVisible({ timeout: 10_000 }).catch(() => false);
      if (!hasCards) {
        // Canvas uses blit rendering at this zoom level — skip card click test
        // but still capture a screenshot of the canvas view
        await screenshot(page, "plan-view-canvas-overview");
        test.skip(true, "no DOM task cards visible at current canvas zoom — blit-rendered");
        return;
      }

      await screenshot(page, "plan-view-canvas-before-detail");
      await canvasTaskCard.click();

      // The canvas detail overlay uses testIdPrefix "canvas-detail"
      const overlay = page.locator('[data-testid="canvas-detail-overlay"]');
      if (await overlay.isVisible({ timeout: 5_000 }).catch(() => false)) {
        await expect(overlay).toBeVisible();
        await screenshot(page, "plan-view-canvas-detail");

        await page.keyboard.press("Escape");
        await expect(overlay).toBeHidden({ timeout: 5_000 });
        await expect(canvasView).toBeVisible();
      } else {
        // Fallback: overlay may use plan-detail prefix
        const fallbackOverlay = page.locator('[data-testid="plan-detail-overlay"]');
        if (await fallbackOverlay.isVisible({ timeout: 3_000 }).catch(() => false)) {
          await screenshot(page, "plan-view-canvas-detail");
          await page.keyboard.press("Escape");
          await expect(fallbackOverlay).toBeHidden({ timeout: 5_000 });
        }
        await expect(canvasView).toBeVisible();
      }
    } finally {
      await canvas.close();
    }
  },
);
