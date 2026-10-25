import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "./suites/12-web",
  testMatch: "sidebar-touch.spec.ts",
  workers: 1,
  retries: 0,
  timeout: 60_000,
  globalTimeout: 120_000,
  reporter: "line",
  outputDir: process.env.SIDEBAR_TOUCH_OUTPUT ?? "./artifacts/sidebar-touch",
  use: {
    browserName: "webkit",
    viewport: { width: 1024, height: 768 },
    hasTouch: true,
    isMobile: true,
    deviceScaleFactor: 2,
    trace: "retain-on-failure",
  },
});
