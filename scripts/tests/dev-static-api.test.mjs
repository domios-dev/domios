import assert from "node:assert/strict";
import { once } from "node:events";
import { createServer } from "node:http";
import { test } from "node:test";
import { isDaemonPath, proxyDevApi } from "../dev-static-server.mjs";

async function listen(server) {
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  return server.address().port;
}

async function close(server) {
  server.closeAllConnections();
  await new Promise((resolve, reject) => server.close((error) => error ? reject(error) : resolve()));
}

test("daemon namespaces never select the SPA fallback", () => {
  for (const prefix of ["/api", "/frontend", "/bundled"]) {
    for (const suffix of ["", "?key=font", "/config", "/missing.js?generation=2"]) {
      assert.equal(isDaemonPath(prefix + suffix), true);
    }
  }
  for (const path of ["/", "/workspace", "/assets/index.js", "/apiary", "/frontend-other", undefined]) {
    assert.equal(isDaemonPath(path), false);
  }
});

test("static routing forwards daemon responses, credentials, query and body without SPA HTML", async () => {
  const calls = [];
  const daemon = createServer(async (req, res) => {
    const chunks = [];
    for await (const chunk of req) chunks.push(chunk);
    calls.push({ path: req.url, method: req.method, token: req.headers.authorization, body: Buffer.concat(chunks).toString() });
    res.writeHead(req.url.startsWith("/api/config") ? 200 : 404, { "content-type": "application/json" });
    res.end(JSON.stringify(req.url.startsWith("/api/config") ? { font: { size: 18 } } : { error: "missing" }));
  });
  const apiPort = await listen(daemon);
  const frontend = createServer((req, res) => {
    if (proxyDevApi(req, res, apiPort)) return;
    res.end("<!doctype html><div id=\"root\"></div>");
  });
  const port = await listen(frontend);
  try {
    const response = await fetch(`http://127.0.0.1:${port}/api/config?key=font`, {
      method: "PATCH", headers: { authorization: "Bearer fixture", "content-type": "application/json" }, body: '{"font":{"size":18}}',
    });
    const config = await response.json();
    assert.equal(config.font.size, 18);
    assert.deepEqual(calls[0], { path: "/api/config?key=font", method: "PATCH", token: "Bearer fixture", body: '{"font":{"size":18}}' });
    for (const path of ["/api/missing", "/frontend/missing/index.html", "/bundled/missing.js"]) {
      const missing = await fetch(`http://127.0.0.1:${port}${path}`);
      assert.equal(missing.status, 404);
      assert.deepEqual(await missing.json(), { error: "missing" });
    }
    const spa = await fetch(`http://127.0.0.1:${port}/workspace`);
    assert.match(await spa.text(), /<!doctype html>/);
    await close(daemon);
    const unavailable = await fetch(`http://127.0.0.1:${port}/api/config`);
    assert.equal(unavailable.status, 502);
    assert.deepEqual(await unavailable.json(), { error: "Dev daemon is unavailable" });
  } finally {
    await close(frontend);
    if (daemon.listening) await close(daemon);
  }
});
