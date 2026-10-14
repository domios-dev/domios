#!/usr/bin/env node
// Caller: skill dev-testing
// Screenshot of a running Domios dev window over its CDP port; no Playwright, no deps.
// Usage: node scripts/dev-screenshot.mjs [out.png] [css-selector] [--api-port 7686]
import { writeFileSync } from "node:fs";

const args = process.argv.slice(2);
const portFlag = args.indexOf("--api-port");
const apiPort = portFlag >= 0 ? args.splice(portFlag, 2)[1] : "7686";
const [out = "dev-screenshot.png", selector = ""] = args;

const health = await (await fetch(`http://127.0.0.1:${apiPort}/api/health`)).json();
const cdpPort = health.instance?.cdp_port;
if (!cdpPort) throw new Error(`instance on API port ${apiPort} exposes no cdp_port`);
const vitePort = health.instance?.vite_port;

const targets = await (await fetch(`http://127.0.0.1:${cdpPort}/json`)).json();
const page = targets.find((t) => t.type === "page" && vitePort && t.url.includes(`:${vitePort}`))
  ?? targets.find((t) => t.type === "page");
if (!page) throw new Error(`no page target on CDP port ${cdpPort}`);

const ws = new WebSocket(page.webSocketDebuggerUrl);
const pending = new Map();
let nextId = 0;
const call = (method, params = {}) => new Promise((resolve) => {
  pending.set(++nextId, resolve);
  ws.send(JSON.stringify({ id: nextId, method, params }));
});
ws.onmessage = (event) => {
  const message = JSON.parse(event.data);
  pending.get(message.id)?.(message.result ?? { error: message.error });
};
await new Promise((resolve) => { ws.onopen = resolve; });

let clip;
if (selector) {
  const { result } = await call("Runtime.evaluate", {
    expression: `(() => { const el = [...document.querySelectorAll(${JSON.stringify(selector)})].find((e) => e.getClientRects().length && e.getBoundingClientRect().width); el?.scrollIntoView({ block: "nearest" }); const r = el?.getBoundingClientRect(); return r && r.width && r.height ? [r.x, r.y, r.width, r.height] : null; })()`,
    returnByValue: true,
  });
  if (!result?.value) throw new Error(`selector not visible: ${selector}`);
  const [x, y, width, height] = result.value;
  clip = { x, y, width, height, scale: 1 };
}

const shot = await call("Page.captureScreenshot", { format: "png", ...(clip ? { clip } : {}) });
ws.close();
if (!shot.data) throw new Error(`capture failed: ${JSON.stringify(shot.error)}`);
writeFileSync(out, Buffer.from(shot.data, "base64"));
console.log(`${out} (build ${health.build}, ${page.url})`);
