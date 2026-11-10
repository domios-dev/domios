import { readFileSync, realpathSync } from "node:fs";
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

const root = process.platform === "win32" && /^[a-z]:[\\/]$/i.test(process.cwd())
  ? realpathSync.native(process.cwd())
  : undefined;

export default defineConfig({
  plugins: [react()],
  test: {
    // Canonicalize drive roots before Pathe resolution and glob directory enumeration.
    root,
    environment: "happy-dom",
    include,
    testTimeout: 60_000,
    hookTimeout: 60_000,
    slowTestThreshold: 1_000,
    fileParallelism: false,
    maxWorkers: 1,
    execArgv,
  },
});
