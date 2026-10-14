// Caller: scripts bench-chat-stream.md
import { build } from "esbuild";
import { chromium } from "@playwright/test";
import { createServer } from "node:http";
import assert from "node:assert/strict";
import { writeFile } from "node:fs/promises";

const bundle = await build({ entryPoints: ["scripts/bench-chat-stream.tsx"], bundle: true, write: false,
  format: "esm", platform: "browser", jsx: "automatic", define: { "process.env.NODE_ENV": '"development"' } });
const server = createServer((req, res) => {
  res.setHeader("Content-Type", req.url === "/bench.js" ? "application/javascript" : "text/html");
  res.end(req.url === "/bench.js" ? bundle.outputFiles[0].text : '<div id="root"></div><script type="module" src="/bench.js"></script>');
});
await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
const browser = await chromium.launch({ headless: true });
let failureDeadline;
const deadline = new Promise((_, reject) => {
  failureDeadline = setTimeout(() => reject(new Error("Streaming benchmark exceeded its 60-second failure deadline")), 60_000);
});
try {
  const page = await browser.newPage();
  await page.goto(`http://127.0.0.1:${server.address().port}`);
  await page.waitForFunction(() => typeof window.runChatStreamBenchmark === "function");
  const cdp = await page.context().newCDPSession(page);
  await cdp.send("Profiler.enable");
  await cdp.send("Profiler.start");
  let complete = false;
  const pings = [];
  const run = page.evaluate((burst) => window.runChatStreamBenchmark(burst ? { chunkSize: 1024, cadence: "task" } : {}), process.argv.includes("--burst"));
  const resultPromise = run.then((result) => ({ result }), (error) => ({ error })).finally(() => { complete = true; });
  while (!complete) {
    const start = performance.now();
    await Promise.race([cdp.send("Runtime.evaluate", { expression: "1" }), deadline]);
    pings.push(performance.now() - start);
  }
  const outcome = await resultPromise;
  if (outcome.error) throw outcome.error;
  const result = outcome.result;
  const { profile } = await cdp.send("Profiler.stop");
  const output = process.argv.slice(2).find((argument) => !argument.startsWith("--"));
  const metrics = { ...result, maxCdpPingMs: Math.max(...pings), pings: pings.length };
  console.log(JSON.stringify(metrics));
  if (output) {
    await writeFile(output, JSON.stringify(metrics));
    await writeFile(`${output}.cpuprofile`, JSON.stringify(profile));
  }
  if (!process.argv.includes("--baseline")) {
    assert.ok(result.maxLongTaskMs <= 200, `Long task ${result.maxLongTaskMs}ms exceeds 200ms`);
    assert.ok(metrics.maxCdpPingMs <= 1000, `CDP unresponsive for ${metrics.maxCdpPingMs}ms`);
  }
} finally {
  clearTimeout(failureDeadline);
  await browser.close();
  await new Promise((resolve) => server.close(resolve));
}
