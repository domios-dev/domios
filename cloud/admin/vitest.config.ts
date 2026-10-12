import { defineConfig } from "vitest/config";
export default defineConfig({
  test: { include: ["test/**/*.test.ts"], fileParallelism: false },
  resolve: { alias: { "server-only": new URL("./test/server-only.ts", import.meta.url).pathname } },
});
