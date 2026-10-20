// Opens a frontend (such as a lane's Vite server) as a web client of the DEV daemon, with the render hook
// installed before React loads. The DEV token is read from the DEV config and never printed.
//   node scripts/dev-perf/open.mjs <frontend-url> --port <browser-cdp-port> [--width 390 --height 844]
import { connectPage, devApiToken, sleep } from "./cdp.mjs";
import { RENDER_HOOK_SOURCE } from "./renderHook.mjs";

const args = process.argv.slice(2);
const option = (name) => { const index = args.indexOf(name); return index >= 0 ? args[index + 1] : undefined; };
const target = args.find((arg) => /^https?:/.test(arg));
if (!target) throw new Error("usage: open.mjs <frontend-url> --port <cdp-port>");

const token = devApiToken();

const origin = new URL(target).origin;
// One page alternates between frontends so A/B runs never measure a throttled background tab.
let page;
for (const match of [origin, "http://127.0.0.1:", "about:blank"]) {
  try {
    page = await connectPage(Number(option("--port")), match);
    break;
  } catch {
    // Try the next candidate page.
  }
}
if (!page) throw new Error("no page to open the frontend in");
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
