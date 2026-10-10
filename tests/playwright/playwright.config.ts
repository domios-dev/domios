import { defineConfig } from "@playwright/test";
import { playwrightWorkerCount } from "./helpers/dev-instance";

const postMortemArtifacts = {
  trace: "retain-on-failure" as const,
  video: "retain-on-failure" as const,
  screenshot: "only-on-failure" as const,
};

const liveSwitchSelected = process.env.PW_SWITCH_PROJECT_SELECTED === "1" || process.argv.some((arg, index, args) =>
  arg === "--project=live-model-switch" || (arg === "--project" && args[index + 1] === "live-model-switch"));
if (liveSwitchSelected) process.env.PW_SWITCH_PROJECT_SELECTED = "1";
if (liveSwitchSelected && !process.env.PW_SWITCH_ARTIFACT_TAG) process.env.PW_SWITCH_ARTIFACT_TAG = String(Date.now());
if (liveSwitchSelected && (process.env.PW_TARGET !== "dev" || !/^[0-9a-f]{40}$/.test(process.env.PW_SWITCH_DEV_SHA ?? ""))) {
  throw new Error("live-model-switch requires PW_TARGET=dev and a full PW_SWITCH_DEV_SHA; it never owns DEV lifecycle");
}
if (liveSwitchSelected && !/^[a-z0-9-]+$/.test(process.env.PW_SWITCH_ARTIFACT_TAG!)) throw new Error("Unsafe switch artifact tag");

/**
 * Serial uses the isolated `playwright` dev instance by default. CT_E2E_WORKERS=N
 * opts into one named isolated dev instance per Playwright parallel worker slot.
 */
export default defineConfig({
  testDir: "./suites",
  workers: playwrightWorkerCount(),
  fullyParallel: false,
  timeout: 90_000,
  expect: { timeout: 10_000 },
  outputDir: liveSwitchSelected ? `./artifacts/model-switch-${process.env.PW_SWITCH_ARTIFACT_TAG}` : "./artifacts/test-results",
  reporter: [
    ["list"],
    ["html", { outputFolder: "./artifacts/html-report", open: "never" }],
  ],
  use: postMortemArtifacts,
  globalSetup: "./helpers/global-setup",
  projects: [
    // Daemon kill/restart tests run only in the serial `restart` project, so
    // every other shard keeps its daemon alive for the whole run.
    { name: "smoke", grep: /@smoke/, grepInvert: /@quarantine|@restart/, retries: 0 },
    // @stub is inverted here: a mock-only result must never report as a golden
    // pass, even if a test carries both tags.
    {
      name: "golden",
      grep: /@golden/,
      grepInvert: /@quarantine|@restart|@live|@perf|@stub/,
      retries: 0,
      timeout: 300_000,
    },
    // Recorded-provider transport (CODETERM_STUB_AGENT_CMD → replay.mjs): real
    // spawn/detection/parse pipeline, no provider credit.
    {
      name: "stub",
      grep: /@stub/,
      grepInvert: /@quarantine|@restart|@live/,
      retries: 0,
      timeout: 300_000,
    },
    // Full excludes @stub because it has no fixture launcher; stub owns deterministic coverage.
    // Cost: full no longer exercises that flow; its verdict relies on the stub project.
    { name: "full", grepInvert: /@quarantine|@restart|@live|@perf|@stub/, retries: 0 },
    // Deterministic latency budgets with hard asserts; metrics.jsonl trend
    // artifact per run. LLM-free (stub transport for churn).
    {
      name: "perf",
      grep: /@perf/,
      grepInvert: /@quarantine/,
      retries: 0,
      timeout: 300_000,
    },
    // Real-provider LLM flows, run on their own cadence outside golden.
    {
      name: "live",
      grep: /@live/,
      grepInvert: /@quarantine|@model-switch/,
      retries: 0,
      timeout: 420_000,
    },
    ...(liveSwitchSelected ? [{
      name: "live-model-switch",
      testMatch: "**/live-model-switch.spec.ts",
      grep: /@model-switch/,
      retries: 0,
      timeout: 3_600_000,
      use: { trace: "on" as const, actionTimeout: 15_000 },
    }] : []),
    {
      name: "restart",
      grep: /@restart/,
      grepInvert: /@quarantine/,
      retries: 0,
      timeout: 300_000,
    },
    { name: "quarantine", grep: /@quarantine/, retries: 0 },
  ],
});
