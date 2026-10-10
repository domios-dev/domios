import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { build } from "esbuild";
import { chromium } from "@playwright/test";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const output = mkdtempSync(path.join(tmpdir(), "domios-web-memory-"));
const baseline = process.argv.includes("--baseline");
const source = readFileSync(path.join(root, "scripts/fixtures/web-memory-probe.jsx"), "utf8");
const bundle = await build({
  stdin: { contents: source, loader: "jsx", resolveDir: root },
  bundle: true, write: false, format: "esm", platform: "browser",
  define: { "import.meta.env": "{}", "process.env.NODE_ENV": '"production"' },
  loader: { ".css": "empty" }, logLevel: "silent",
});
const server = createServer((request, response) => {
  const script = request.url === "/bundle.js";
  response.setHeader("Content-Type", script ? "text/javascript" : "text/html");
  response.end(script ? bundle.outputFiles[0].text : '<!doctype html><script type="module" src="/bundle.js"></script>');
});
let browser;
const curves = [];
const started = performance.now();

function processMemory(processes) {
  const ids = processes.map((process) => Number(process.id)).join(",");
  if (process.platform === "win32") {
    return JSON.parse(execFileSync("powershell", ["-NoProfile", "-Command",
      `Get-Process -Id ${ids} | Select-Object Id,WorkingSet64,PrivateMemorySize64 | ConvertTo-Json -Compress`,
    ], { encoding: "utf8" }));
  }
  return execFileSync("ps", ["-o", "pid=,rss=", "-p", ids], { encoding: "utf8" })
    .trim().split("\n").map((line) => {
      const [id, rss] = line.trim().split(/\s+/).map(Number);
      return { Id: id, WorkingSet64: rss * 1024 };
    });
}

try {
  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });
  const port = server.address().port;
  assert(port > 9000, "Harness requires a free port above 9000");
  browser = await chromium.launch({ headless: true, args: ["--enable-precise-memory-info"] });
  const page = await browser.newPage();
  const pageErrors = [];
  page.on("pageerror", (error) => pageErrors.push(String(error)));
  await page.goto(`http://127.0.0.1:${port}`);
  await page.waitForFunction(() => window.ready);
  const cdp = await page.context().newCDPSession(page);
  await cdp.send("HeapProfiler.enable");
  const browserCdp = await browser.newBrowserCDPSession();

  async function sample(phase, state = {}) {
    await cdp.send("HeapProfiler.collectGarbage");
    const heap = await cdp.send("Runtime.getHeapUsage");
    const processes = (await browserCdp.send("SystemInfo.getProcessInfo")).processInfo;
    const sample = { phase, elapsedMs: performance.now() - started, ...state, ...heap,
      memory: processMemory(processes), processes };
    curves.push(sample);
    console.log(JSON.stringify({ phase, heapMiB: heap.usedSize / 1024 ** 2, rows: state.rows }));
  }
  async function snapshot(name) {
    const chunks = [];
    const receive = ({ chunk }) => chunks.push(chunk);
    cdp.on("HeapProfiler.addHeapSnapshotChunk", receive);
    try {
      await cdp.send("HeapProfiler.takeHeapSnapshot", { reportProgress: false });
    } finally {
      cdp.off("HeapProfiler.addHeapSnapshotChunk", receive);
    }
    writeFileSync(path.join(output, `${name}.heapsnapshot`), chunks.join(""));
  }

  await sample(0);
  await snapshot("start");
  for (let phase = 1; phase <= 32; phase++) {
    const state = await page.evaluate((phase) => window.step(phase), phase);
    await sample(phase, state);
    if ([4, 8, 32].includes(phase)) await snapshot(String(phase));
    if (!baseline) assert(state.rows.every((count) => count <= 440), "Transcript rows exceeded retention");
  }
  await page.evaluate(() => window.cleanup());
  await sample("cleanup");
  await snapshot("cleanup");
  assert.deepEqual(pageErrors, []);
  if (!baseline) {
    assert(curves[32].usedSize <= curves[4].usedSize + 12 * 1024 ** 2, "Retained heap keeps growing after warm-up");
    assert(curves.at(-1).usedSize < curves[32].usedSize / 2, "Cleanup did not release transcript payloads");
  }
} finally {
  writeFileSync(path.join(output, "curve.json"), JSON.stringify(curves, null, 2));
  console.log(`Memory probe artifacts: ${output}`);
  await browser?.close();
  await new Promise((resolve) => server.close(resolve));
}
