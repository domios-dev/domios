import { defineConfig } from "@playwright/test";

const postMortemArtifacts = {
  trace: "retain-on-failure-and-retries" as const,
  video: "retain-on-failure" as const,
  screenshot: "only-on-failure" as const,
};

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
  outputDir: "./artifacts/test-results",
  reporter: [
    ["list"],
    ["html", { outputFolder: "./artifacts/html-report", open: "never" }],
  ],
  use: postMortemArtifacts,
  globalSetup: "./helpers/global-setup",
  projects: [
    { name: "smoke", grep: /@smoke/, retries: 0 },
    {
      name: "golden",
      grep: /@golden/,
      retries: 1,
      timeout: 300_000,
    },
    { name: "full", grepInvert: /@quarantine/, retries: 1 },
  ],
});
