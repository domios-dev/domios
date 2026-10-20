// Lists the daemon requests that stay open in the DEV page; browsers allow about six per origin,
// so long-lived ones starve every other request.
//   node scripts/dev-perf/connections.mjs [--seconds 30] [--port 9223]
import { APP_READY, connectPage, reloadWithScript, sleep } from "./cdp.mjs";

const args = process.argv.slice(2);
const option = (name) => { const index = args.indexOf(name); return index >= 0 ? args[index + 1] : undefined; };
const seconds = Number(option("--seconds") ?? 30);
const page = await connectPage(option("--port") ? Number(option("--port")) : undefined);

const TRACKER = String.raw`(() => {
  const open = new Map();
  let sequence = 0;
  window.__devPerfRequests = open;
  const original = window.fetch.bind(window);
  window.fetch = (input, init) => {
    const url = (typeof input === "string" ? input : input.url).replace(/^https?:\/\/[^/]+/, "").replace(/(token|since)=[^&]+/g, "$1=*");
    const id = ++sequence;
    open.set(id, { url, started: performance.now(), stream: false });
    const settle = () => open.delete(id);
    init?.signal?.addEventListener("abort", settle, { once: true });
    const pending = original(input, init);
    pending.then((response) => {
      if ((response.headers.get("content-type") || "").includes("event-stream")) open.get(id) && (open.get(id).stream = true);
      else response.clone().arrayBuffer().finally(settle);
    }, settle);
    return pending;
  };
})();`;

await reloadWithScript(page, TRACKER, APP_READY);
for (let elapsed = 0; elapsed < seconds; elapsed += 5) {
  const open = await page.evaluate(`(() => { const now = performance.now(); return [...(window.__devPerfRequests || new Map()).values()]
    .filter((request) => now - request.started > 3000)
    .map((request) => ((now - request.started) / 1000 | 0) + "s " + (request.stream ? "stream " : "") + request.url.slice(0, 120)); })()`);
  console.log(`t+${elapsed}s open longer than 3s: ${open.length}${open.length ? "\n  " + open.join("\n  ") : ""}`);
  await sleep(5000);
}
page.close();
