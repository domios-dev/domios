import assert from "node:assert/strict";
import http from "node:http";
import test from "node:test";
import { MESH_DAEMON_BASE_ENV } from "./daemon-base";
import { MeshStub } from "./mesh-stub";

function acceptRecorder(): { server: http.Server; accepted: () => string | null } {
  let acceptedRequestId: string | null = null;
  const server = http.createServer((request, response) => {
    let body = "";
    request.on("data", (chunk) => (body += chunk));
    request.on("end", () => {
      acceptedRequestId = (JSON.parse(body) as { request_id: string }).request_id;
      response.writeHead(200, { "Content-Type": "application/json" });
      response.end('{"ok":true}');
    });
  });
  return { server, accepted: () => acceptedRequestId };
}

async function closeServer(server: http.Server): Promise<void> {
  server.closeAllConnections();
  await new Promise<void>((resolve) => server.close(() => resolve()));
}

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

async function assertPairAcceptLandsOn(
  stub: MeshStub,
  accepted: () => string | null,
  requestId: string,
): Promise<void> {
  await stub.start();
  try {
    const response = await fetch(`http://127.0.0.1:${stub.port}/api/mesh/pair/request`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ request_id: requestId }),
    });
    assert.equal(response.status, 200);
    assert.deepEqual(await response.json(), { ok: true, instance_id: stub.instanceId });
    await stub.waitForPairAccept(2_000);
    assert.equal(accepted(), requestId);
  } finally {
    await stub.stop();
  }
}

test("pair request completes its callback against the configured daemon", async () => {
  const { server: daemon, accepted } = acceptRecorder();
  const daemonPort = await listen(daemon);
  const stub = new MeshStub({ daemonBase: `http://127.0.0.1:${daemonPort}` });
  try {
    await assertPairAcceptLandsOn(stub, accepted, "pair-request-1");
    assert.equal(stub.daemonBase, `http://127.0.0.1:${daemonPort}`);
  } finally {
    await closeServer(daemon);
  }
});

test("with no explicit base the callback lands on the env-resolved daemon", async () => {
  const { server: daemon, accepted } = acceptRecorder();
  const daemonPort = await listen(daemon);
  const previous = process.env[MESH_DAEMON_BASE_ENV];
  process.env[MESH_DAEMON_BASE_ENV] = `http://127.0.0.1:${daemonPort}`;
  try {
    const stub = new MeshStub({ services: 3 });
    assert.equal(stub.daemonBase, `http://127.0.0.1:${daemonPort}`);
    await assertPairAcceptLandsOn(stub, accepted, "pair-request-env");
  } finally {
    if (previous === undefined) delete process.env[MESH_DAEMON_BASE_ENV];
    else process.env[MESH_DAEMON_BASE_ENV] = previous;
    await closeServer(daemon);
  }
});

// Negative control for the original defect: a services-only construction used
// to silently address 7686, the INTERACTIVE dev console.
test("the interactive dev console port is refused at construction", () => {
  assert.throws(
    () => new MeshStub({ services: 3, daemonBase: "http://127.0.0.1:7686" }),
    /refusing harness daemon base on shared instance port 7686/,
  );
});

test("an unresolvable env base fails construction instead of defaulting", () => {
  const previous = process.env[MESH_DAEMON_BASE_ENV];
  process.env[MESH_DAEMON_BASE_ENV] = "not-a-url";
  try {
    assert.throws(() => new MeshStub({ services: 3 }), /not an absolute URL/);
  } finally {
    if (previous === undefined) delete process.env[MESH_DAEMON_BASE_ENV];
    else process.env[MESH_DAEMON_BASE_ENV] = previous;
  }
});
