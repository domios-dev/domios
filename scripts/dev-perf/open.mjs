// Opens a frontend (such as a lane's Vite server) as a web client of the DEV daemon, with the render hook
// installed before React loads. The DEV token is read from the DEV config and never printed.
//   node scripts/dev-perf/open.mjs <frontend-url> --port <browser-cdp-port> [--width 390 --height 844]
import { readFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import { connectPage, sleep } from "./cdp.mjs";
import { RENDER_HOOK_SOURCE } from "./renderHook.mjs";

const args = process.argv.slice(2);
const option = (name) => { const index = args.indexOf(name); return index >= 0 ? args[index + 1] : undefined; };
const target = args.find((arg) => /^https?:/.test(arg));
if (!target) throw new Error("usage: open.mjs <frontend-url> --port <cdp-port>");

const config = readFileSync(join(homedir(), ".codeterm-dev", "config.toml"), "utf8");
const apiSection = config.slice(config.indexOf("[api]"));
const token = apiSection.match(/^token = "([^"]+)"/m)?.[1];
if (!token) throw new Error("DEV config has no [api] token");

const origin = new URL(target).origin;
let page;
try {
  page = await connectPage(Number(option("--port")), origin);
} catch {
  page = await connectPage(Number(option("--port")), "about:blank");
}
await page.call("Page.enable");
const width = Number(option("--width") ?? 0);
if (width) {
  await page.call("Emulation.setDeviceMetricsOverride", { width, height: Number(option("--height") ?? 844), deviceScaleFactor: 3, mobile: true });
  await page.call("Emulation.setTouchEmulationEnabled", { enabled: true, maxTouchPoints: 5 });
} else {
  await page.call("Emulation.clearDeviceMetricsOverride");
  await page.call("Emulation.setTouchEmulationEnabled", { enabled: false });
}
await page.call("Page.addScriptToEvaluateOnNewDocument", { source: RENDER_HOOK_SOURCE });
const url = `${target.replace(/#.*$/, "")}#token=${encodeURIComponent(token)}`;
const current = await page.evaluate("location.href");
// A hash-only navigation keeps the document, so the hook would never run; reload instead.
if (current.replace(/#.*$/, "") === url.replace(/#.*$/, "")) await page.call("Page.reload", {});
else await page.call("Page.navigate", { url });
for (let attempt = 0; attempt < 120; attempt++) {
  await sleep(500);
  const ready = await page.evaluate("!!window.__devPerfRenders && document.querySelectorAll('[data-tab-id]').length > 0").catch(() => false);
  if (ready) {
    console.log(`open: ${origin} ready with render hook${width ? ` at ${width}px` : ""}`);
    page.close();
    process.exit(0);
  }
}
throw new Error("frontend did not render tab rows");
