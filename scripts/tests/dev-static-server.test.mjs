#!/usr/bin/env node
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { request } from "node:http";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawn } from "node:child_process";

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

function freePort() {
  return new Promise((resolve, reject) => {
    const server = request;
    import("node:net").then(({ createServer }) => {
      const s = createServer();
      s.listen(0, "127.0.0.1", () => {
        const address = s.address();
        s.close(() => resolve(address.port));
      });
      s.on("error", reject);
    }).catch(reject);
    void server;
  });
}

function httpGet(port) {
  return new Promise((resolve, reject) => {
    const req = request({ host: "127.0.0.1", port, path: "/", timeout: 300 }, (res) => {
      res.resume();
      res.on("end", () => resolve(res.statusCode));
    });
    req.on("timeout", () => {
      req.destroy(new Error("timeout"));
    });
    req.on("error", reject);
    req.end();
  });
}

async function waitForStatus(port, expected) {
  const deadline = Date.now() + 5000;
  let lastError = null;
  while (Date.now() < deadline) {
    try {
      const status = await httpGet(port);
      if (status === expected) return;
      lastError = new Error(`status ${status}`);
    } catch (error) {
      lastError = error;
    }
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  throw lastError ?? new Error("server did not answer");
}

const tmp = mkdtempSync(join(tmpdir(), "codeterm-dev-static-"));
const dist = join(tmp, "dist");
mkdirSync(dist);
const port = await freePort();
const child = spawn(process.execPath, ["scripts/dev-static-server.mjs"], {
  cwd: join(import.meta.dirname, "..", ".."),
  env: {
    ...process.env,
    CODETERM_DEV_STATIC_DIST: dist,
    CODETERM_VITE_PORT: String(port),
    CODETERM_DEV_STATIC_TIMEOUT_MS: "5000",
    CODETERM_DEV_STATIC_POLL_MS: "50",
  },
  stdio: ["ignore", "pipe", "pipe"],
});

let stdout = "";
let stderr = "";
child.stdout.on("data", (chunk) => {
  stdout += chunk;
});
child.stderr.on("data", (chunk) => {
  stderr += chunk;
});

try {
  await new Promise((resolve) => setTimeout(resolve, 250));
  let connectedEarly = false;
  try {
    await httpGet(port);
    connectedEarly = true;
  } catch {
    connectedEarly = false;
  }
  assert(!connectedEarly, "server must not listen before dist/index.html exists");

  writeFileSync(join(dist, "index.html"), "<!doctype html><div id=\"root\"></div>");
  await waitForStatus(port, 200);
  assert(stdout.includes("[dev-static] waiting for"), "server should log the wait state");
  assert(stdout.includes("[dev-static] serving"), "server should begin serving after index exists");
  console.log("  ✓ dev-static waits for cold dist before listening");
} finally {
  child.kill();
  rmSync(tmp, { recursive: true, force: true });
}

child.on("exit", (code) => {
  if (code && code !== 0 && code !== null) {
    console.error(stdout);
    console.error(stderr);
    process.exit(code);
  }
});
