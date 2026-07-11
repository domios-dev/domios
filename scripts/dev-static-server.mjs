#!/usr/bin/env node
import { createReadStream, existsSync, statSync } from "node:fs";
import { createServer } from "node:http";
import { extname, join, normalize, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

const repo = resolve(fileURLToPath(new URL("..", import.meta.url)));
const dist = process.env.CODETERM_DEV_STATIC_DIST
  ? resolve(process.env.CODETERM_DEV_STATIC_DIST)
  : join(repo, "dist");
const index = join(dist, "index.html");
const port = Number(process.env.CODETERM_VITE_PORT) || 1420;
const startupTimeoutMs = Number(process.env.CODETERM_DEV_STATIC_TIMEOUT_MS) || 120_000;
const pollMs = Number(process.env.CODETERM_DEV_STATIC_POLL_MS) || 250;

const types = new Map([
  [".css", "text/css"],
  [".html", "text/html"],
  [".ico", "image/x-icon"],
  [".js", "text/javascript"],
  [".json", "application/json"],
  [".png", "image/png"],
  [".svg", "image/svg+xml"],
  [".woff2", "font/woff2"],
]);

function resolveAsset(urlPath) {
  const raw = decodeURIComponent((urlPath || "/").split("?")[0]);
  const requested = normalize(raw === "/" ? "/index.html" : raw);
  const full = resolve(join(dist, requested));
  if (!full.startsWith(dist + sep) && full !== dist) return null;
  if (existsSync(full) && statSync(full).isFile()) return full;
  return index;
}

function indexReady() {
  try {
    return statSync(index).isFile();
  } catch {
    return false;
  }
}

function delay(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

async function waitForIndex() {
  const start = Date.now();
  while (!indexReady()) {
    if (Date.now() - start > startupTimeoutMs) {
      throw new Error(`timed out waiting for ${index}`);
    }
    await delay(pollMs);
  }
}

export async function startServer() {
  if (!indexReady()) {
    console.log(`[dev-static] waiting for ${index}`);
    await waitForIndex();
  }
  return createServer((req, res) => {
    const file = resolveAsset(req.url);
    if (!file || !existsSync(file)) {
      res.writeHead(404, { "cache-control": "no-cache" });
      res.end("dist/index.html missing");
      return;
    }
    res.writeHead(200, {
      "cache-control": "no-cache",
      "content-type": types.get(extname(file)) || "application/octet-stream",
    });
    createReadStream(file).pipe(res);
  }).listen(port, "127.0.0.1", () => {
    console.log(`[dev-static] serving ${dist} on http://127.0.0.1:${port}/`);
  });
}

if (fileURLToPath(import.meta.url) === resolve(process.argv[1])) {
  startServer().catch((error) => {
    console.error(`[dev-static] ${error?.message || error}`);
    process.exit(1);
  });
}
