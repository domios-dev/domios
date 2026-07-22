import assert from "node:assert/strict";
import http from "node:http";
import test from "node:test";
import { MeshStub } from "./mesh-stub";

function listen(server: http.Server): Promise<number> {
  return new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      if (!address || typeof address === "string") reject(new Error("server did not expose a TCP port"));
      else resolve(address.port);
    });
  });
}

test("pair request completes its callback against the configured daemon", async () => {
  let acceptedRequestId: string | null = null;
  const daemon = http.createServer((request, response) => {
    let body = "";
    request.on("data", (chunk) => (body += chunk));
    request.on("end", () => {
      acceptedRequestId = (JSON.parse(body) as { request_id: string }).request_id;
      response.writeHead(200, { "Content-Type": "application/json" });
      response.end('{"ok":true}');
    });
  });
  const daemonPort = await listen(daemon);
  const stub = new MeshStub({ daemonBase: `http://127.0.0.1:${daemonPort}` });
  await stub.start();

  try {
    const response = await fetch(`http://127.0.0.1:${stub.port}/api/mesh/pair/request`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ request_id: "pair-request-1" }),
    });
    assert.equal(response.status, 200);
    assert.deepEqual(await response.json(), { ok: true, instance_id: stub.instanceId });
    await stub.waitForPairAccept(2_000);
    assert.equal(acceptedRequestId, "pair-request-1");
  } finally {
    await stub.stop();
    daemon.closeAllConnections();
    await new Promise<void>((resolve) => daemon.close(() => resolve()));
  }
});
