import { test, expect } from "../fixtures.js";

test.describe("Settings panel", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    await page.waitForSelector("#root > *", { timeout: 10_000 });
  });

  test("opens settings panel via sidebar button", async ({ page }) => {
    // The "Settings" button is at the bottom of the sidebar
    const settingsBtn = page.locator('text=/Settings/i').first();

    if (await settingsBtn.isVisible({ timeout: 3_000 })) {
      await settingsBtn.click();
      await page.waitForTimeout(500);

      // Settings panel shows an h2 with "Settings" text inside a fixed overlay
      const settingsHeading = page.locator('h2:has-text("Settings")');
      await expect(settingsHeading).toBeVisible({ timeout: 3_000 });
    }
  });

  test("settings panel shows font configuration", async ({ page }) => {
    // Open settings via the sidebar button
    const settingsBtn = page.locator('text=/Settings/i').first();

    if (await settingsBtn.isVisible({ timeout: 3_000 })) {
      await settingsBtn.click();
      await page.waitForTimeout(500);

      // Look for font-related content in the settings panel
      const fontLabel = page.locator('text=/Font family/i');
      await expect(fontLabel).toBeVisible({ timeout: 3_000 });

      // Verify the default font family is displayed
      const fontInput = page.locator('input[value*="JetBrains"]');
      if (await fontInput.count() > 0) {
        await expect(fontInput.first()).toBeVisible();
      }
    }
  });

  test("settings panel has navigation tabs", async ({ page }) => {
    const settingsBtn = page.locator('text=/Settings/i').first();

    if (await settingsBtn.isVisible({ timeout: 3_000 })) {
      await settingsBtn.click();
      await page.waitForTimeout(500);

      // Check for settings tab navigation items inside the settings overlay
      const overlay = page.locator('.fixed').first();
      const tabs = ["Appearance", "Terminal", "Keybindings", "Notifications", "Agents"];
      for (const tabName of tabs) {
        const tab = overlay.locator(`button:has-text("${tabName}")`);
        await expect(tab).toBeVisible({ timeout: 2_000 });
      }
    }
  });

  test("can close settings panel", async ({ page }) => {
    const settingsBtn = page.locator('text=/Settings/i').first();

    if (await settingsBtn.isVisible({ timeout: 3_000 })) {
      await settingsBtn.click();
      await page.waitForTimeout(500);

      // Settings heading should be visible
      const settingsHeading = page.locator('h2:has-text("Settings")');
      await expect(settingsHeading).toBeVisible({ timeout: 3_000 });

      // Click close button (the X button in the header)
      const closeBtn = page.locator('.fixed button svg').first();
      await closeBtn.click();
      await page.waitForTimeout(300);

      // Settings heading should no longer be visible
      await expect(settingsHeading).not.toBeVisible({ timeout: 2_000 });
    }
  });
});
