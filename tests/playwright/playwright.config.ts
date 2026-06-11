import { defineConfig } from "@playwright/test";

/**
 * One app instance, one worker — every suite drives the same live dev
 * console (port 7686) over CDP + REST. Parallel workers would fight over
 * focus and global mux state.
 */
export default defineConfig({
  testDir: "./suites",
  workers: 1,
  fullyParallel: false,
  timeout: 90_000,
  expect: { timeout: 10_000 },
  retries: 0,
  reporter: [["list"]],
  globalSetup: "./helpers/global-setup",
  projects: [
    { name: "smoke", grep: /@smoke/ },
    { name: "golden", grep: /@golden/, retries: 1, timeout: 300_000 },
    { name: "full", grepInvert: /@quarantine/ },
  ],
});
