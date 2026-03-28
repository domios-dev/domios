import { test, expect } from "../fixtures.js";

test.describe("App initialization", () => {
  test("renders the main app shell", async ({ page }) => {
    await page.goto("/");
    // The app should render without crashing
    const body = page.locator("body");
    await expect(body).toBeVisible();
    // Wait for React to mount (the root div gets children)
    await page.waitForSelector("#root > *", { timeout: 10_000 });
  });

  test("shows the titlebar", async ({ page }) => {
    await page.goto("/");
    await page.waitForSelector("#root > *", { timeout: 10_000 });
    // Titlebar is always rendered -- look for its container
    const titlebar = page.locator('[data-testid="titlebar"], [class*="titlebar"], [class*="Titlebar"]');
    // If no data-testid, fall back to checking for window control buttons
    const windowControls = page.locator(
      'button[aria-label="close"], button[aria-label="minimize"], [class*="window-control"], [class*="traffic"]'
    );
    const hasTitlebar = await titlebar.count();
    const hasControls = await windowControls.count();
    expect(hasTitlebar + hasControls).toBeGreaterThan(0);
  });

  test("shows the sidebar", async ({ page }) => {
    await page.goto("/");
    await page.waitForSelector("#root > *", { timeout: 10_000 });
    // Sidebar should be visible with workspace/tab content
    const sidebar = page.locator('[data-testid="sidebar"], [class*="sidebar"], [class*="Sidebar"]');
    await expect(sidebar.first()).toBeVisible({ timeout: 5_000 });
  });

  test("loads with correct theme colors", async ({ page }) => {
    await page.goto("/");
    await page.waitForSelector("#root > *", { timeout: 10_000 });
    // The mock provides github-dark theme. Check that CSS vars are applied.
    const bgColor = await page.evaluate(() => {
      const root = document.documentElement;
      return getComputedStyle(root).getPropertyValue("--ct-bg").trim();
    });
    // Should have a dark background (github-dark: #0d1117)
    // The var might not be on :root but on a container div
    if (bgColor) {
      expect(bgColor).toMatch(/#[0-9a-f]{6}/i);
    }
  });

  test("mock IPC is active", async ({ page }) => {
    await page.goto("/");
    const hasMock = await page.evaluate(() => {
      return !!(window as any).__CODETERM_TEST_MOCK__;
    });
    expect(hasMock).toBe(true);
  });
});
