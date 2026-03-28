import { test as base, expect } from "@playwright/test";
import { join } from "path";

const tauriMockPath = join(import.meta.dirname, "tauri-mock.js");

/**
 * Extended test fixture that injects the Tauri IPC mock before each page load.
 * The mock lives in tauri-mock.js (plain JS, no TS compilation needed).
 */
export const test = base.extend({
  page: async ({ page }, use) => {
    // Inject Tauri mock before any script runs on the page
    await page.addInitScript({ path: tauriMockPath });

    // Collect console errors for debugging
    const errors: string[] = [];
    page.on("pageerror", (err) => {
      errors.push(err.message);
    });
    page.on("console", (msg) => {
      if (msg.type() === "error") {
        errors.push(msg.text());
      }
    });

    await use(page);

    // Log errors after test for debugging (visible in --reporter=line output)
    if (errors.length > 0) {
      console.log("[test console errors]", errors.slice(0, 5).join("\n"));
    }
  },
});

export { expect };
