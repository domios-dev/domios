import { readFileSync } from "node:fs";
import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

// Single source of truth: the node unit runner skips exactly these files.
const include = JSON.parse(
  readFileSync(new URL("./scripts/vitest-owned-tests.json", import.meta.url), "utf8"),
) as string[];

// Node 25's global localStorage lacks its methods without --localstorage-file and shadows happy-dom's.
const execArgv = process.allowedNodeEnvironmentFlags.has("--no-experimental-webstorage")
  ? ["--no-experimental-webstorage"]
  : [];

export default defineConfig({
  plugins: [react()],
  test: {
    environment: "happy-dom",
    include,
    poolOptions: { forks: { execArgv } },
  },
});
