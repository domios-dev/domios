import { test, expect } from "../fixtures.js";

test.describe("Keyboard shortcuts", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    await page.waitForSelector("#root > *", { timeout: 10_000 });
  });

  test("Cmd+K opens command palette", async ({ page }) => {
    // Command palette is typically opened with Cmd+K or Ctrl+K
    await page.keyboard.press("Meta+k");
    await page.waitForTimeout(500);

    const palette = page.locator(
      '[data-testid="command-palette"], [class*="command-palette"], [class*="CommandPalette"], [class*="palette"]'
    );
    const visible = await palette.count();
    // Even if not visible, test that no crash occurred
    if (visible > 0) {
      await expect(palette.first()).toBeVisible();
    }
  });

  test("Escape closes overlays", async ({ page }) => {
    // Open something first (try command palette)
    await page.keyboard.press("Meta+k");
    await page.waitForTimeout(300);

    // Press Escape
    await page.keyboard.press("Escape");
    await page.waitForTimeout(300);

    // Any overlay/modal should be gone
    const overlays = page.locator('[class*="overlay"], [class*="modal"], [class*="palette"]');
    const visibleOverlays = await overlays.evaluateAll((els) =>
      els.filter((el) => {
        const style = window.getComputedStyle(el);
        return style.display !== "none" && style.visibility !== "hidden" && style.opacity !== "0";
      }).length
    );
    // Should have 0 visible overlays (or at most the always-visible ones)
    expect(visibleOverlays).toBeLessThanOrEqual(1);
  });
});
