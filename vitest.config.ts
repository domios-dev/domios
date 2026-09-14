import { readFileSync } from "node:fs";
import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

// Single source of truth: the node unit runner skips exactly these files.
const include = JSON.parse(
  readFileSync(new URL("./scripts/vitest-owned-tests.json", import.meta.url), "utf8"),
) as string[];

export default defineConfig({
  plugins: [react()],
  test: {
    environment: "happy-dom",
    include,
  },
});
