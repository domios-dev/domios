import { defineConfig, devices } from "@playwright/test";

/**
 * Playwright config for CodeTerm UI E2E tests.
 *
 * The tests run against the Vite dev server (port 1420) with a Tauri IPC mock
 * injected into every page. This lets us test the real React UI -- layout,
 * clicks, navigation, theming -- without needing the native Tauri shell.
 *
 * For full integration tests that also exercise PTY/backend logic, the Tauri
 * app must be running and the REST API (port 7686) is used for verification.
 */
export default defineConfig({
  testDir: "./specs",
  fullyParallel: false,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 1 : 0,
  workers: 1,
  reporter: [["html", { open: "never" }], ["list"]],
  timeout: 30_000,

  use: {
    baseURL: "http://localhost:1420",
    trace: "on-first-retry",
    screenshot: "only-on-failure",
  },

  projects: [
    {
      name: "webkit",
      use: { ...devices["Desktop Safari"] },
    },
    {
      name: "chromium",
      use: { ...devices["Desktop Chrome"] },
    },
  ],

  /* Start the Vite dev server if it isn't already running. */
  webServer: {
    command: "pnpm dev --host",
    cwd: "../..",
    port: 1420,
    reuseExistingServer: true,
    timeout: 30_000,
  },
});
