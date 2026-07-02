/**
 * 13-canvas — plan view mode shell: Board | List | Canvas top switch.
 */
import { expect, test } from "../../helpers/fixtures";
import {
  isEmpty,
  seedPerfFixtureAndOpenInlineCanvas,
} from "../../helpers/canvas-inspect";

test("@golden @plan-view-mode plan surface exposes board list canvas switch", async ({ api, webBrowser }) => {
  test.setTimeout(120_000);
  const canvas = await seedPerfFixtureAndOpenInlineCanvas(webBrowser, api);
  try {
    const { page } = canvas;
    if (await isEmpty(page)) {
      test.skip(true, "canvas empty — perf fixture did not inject");
      return;
    }

    await page.evaluate(() => window.localStorage.removeItem("codeterm.planViewMode"));

    const shell = page.locator('[data-testid="plan-view-shell"]');
    await expect(shell).toBeVisible({ timeout: 30_000 });
    await expect(shell).toHaveAttribute("data-plan-view-mode", "board");

    const modeSwitch = page.locator('[data-testid="plan-view-mode-switch"]');
    await expect(modeSwitch).toBeVisible();
    await expect(modeSwitch.getByRole("tab", { name: "Board", selected: true })).toBeVisible();
    await expect(modeSwitch.getByRole("tab", { name: "List" })).toBeVisible();
    await expect(modeSwitch.getByRole("tab", { name: "Canvas" })).toBeVisible();
    await expect(page.locator('[data-testid="plan-board-view"]')).toBeVisible();

    await modeSwitch.getByRole("tab", { name: "List" }).click();
    await expect(shell).toHaveAttribute("data-plan-view-mode", "list");
    await expect(page.locator('[data-testid="plan-list-view"]')).toBeVisible();
    await expect(page.locator('[data-testid="plan-board-view"]')).toBeHidden();

    await modeSwitch.getByRole("tab", { name: "Canvas" }).click();
    await expect(shell).toHaveAttribute("data-plan-view-mode", "canvas");
    await expect(page.locator('[data-testid="canvas-view"]')).toBeVisible();
  } finally {
    await canvas.close();
  }
});
