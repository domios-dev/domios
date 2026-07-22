#!/usr/bin/env node
import { createServer } from "node:http";
import { spawnSync } from "node:child_process";

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

function listen(server) {
  return new Promise((resolve, reject) => {
    server.listen(0, "127.0.0.1", () => resolve(server.address().port));
    server.on("error", reject);
  });
}

async function fakeHttpCdp(targets) {
  const http = createServer((req, res) => {
    if (req.url === "/json/list") {
      res.writeHead(200, { "content-type": "application/json" });
      res.end(JSON.stringify(targets));
      return;
    }
    res.writeHead(404);
    res.end();
  });
  const port = await listen(http);
  return {
    port,
    close: () => new Promise((resolve) => http.close(resolve)),
  };
}

function runProbe(port, vitePort) {
  return spawnSync(process.execPath, ["scripts/dev-frontend-ready.mjs", "ready"], {
    cwd: new URL("../..", import.meta.url),
    env: {
      ...process.env,
      CODETERM_CDP_PORT: String(port),
      CODETERM_VITE_PORT: String(vitePort),
    },
    encoding: "utf8",
  });
}

async function testNoTargetFails() {
  const cdp = await fakeHttpCdp([]);
  try {
    const result = runProbe(cdp.port, 1420);
    assert(result.status === 1, `no target should exit 1, got ${result.status}`);
  } finally {
    await cdp.close();
  }
}

async function testWrongUrlFails() {
  const cdp = await fakeHttpCdp([{
    type: "page",
    url: "http://127.0.0.1:9999/",
    webSocketDebuggerUrl: `ws://127.0.0.1:9/devtools/page/test`,
  }]);
  try {
    const result = runProbe(cdp.port, 1420);
    assert(result.status === 1, `wrong URL should exit 1, got ${result.status}`);
  } finally {
    await cdp.close();
  }
}

async function testLiveCdpWhenAvailable() {
  const cdpPort = process.env.CODETERM_CDP_PORT;
  const vitePort = process.env.CODETERM_VITE_PORT;
  if (!cdpPort || !vitePort) {
    console.log("  - live CDP skipped (CODETERM_CDP_PORT/CODETERM_VITE_PORT unset)");
    return;
  }
  const result = runProbe(cdpPort, vitePort);
  assert(result.status === 0, `live CDP ready should exit 0, got ${result.status}: ${result.stderr}`);
}

const tests = [
  ["no target fails", testNoTargetFails],
  ["wrong URL fails", testWrongUrlFails],
  ["live CDP ready when available", testLiveCdpWhenAvailable],
];

let failed = 0;
for (const [name, fn] of tests) {
  try {
    await fn();
    console.log(`  ✓ ${name}`);
  } catch (error) {
    failed++;
    console.error(`  ✗ ${name}`);
    console.error(error);
  }
}
if (failed > 0) process.exit(1);
console.log(`\n✓ ${tests.length}/${tests.length} dev-frontend-ready tests passed`);
