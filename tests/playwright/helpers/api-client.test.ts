/**
 * WHAT: ApiClient supports bounded route-specific request timeouts.
 * WHY: Real orchestration launch legitimately exceeds the default API budget.
 * ORACLE: A slow route fails at its selected bound with actionable diagnostics.
 */
import assert from "node:assert/strict";
import { createServer } from "node:http";
import test from "node:test";
import { ApiClient } from "./api-client";
import { devInstanceForSlot } from "./dev-instance";

test("route-specific timeout reports the method, path, and bound", async () => {
  const server = createServer(() => {});
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  try {
    const address = server.address();
    assert(address && typeof address === "object");
    const base = devInstanceForSlot(0);
    const apiBase = `http://127.0.0.1:${address.port}`;
    const client = new ApiClient("test-token", { ...base, apiPort: address.port, apiBase });

    await assert.rejects(
      client.post("/slow-pair", {}, { timeoutMs: 25 }),
      /POST \/api\/slow-pair timed out after 25ms/,
    );
    await assert.rejects(
      client.get("/invalid", { timeoutMs: 0 }),
      /invalid API request timeout: 0/,
    );
  } finally {
    server.closeAllConnections();
    await new Promise<void>((resolve, reject) =>
      server.close((error) => (error ? reject(error) : resolve())),
    );
  }
});
