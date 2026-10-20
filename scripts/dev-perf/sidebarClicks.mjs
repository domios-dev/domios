// Clicks sidebar tab rows with the sidebar scrolled away from the top and records what the user sees:
// whether the clicked row is the one that activates, sidebar scroll jumps, row displacement, drag starts
// and long tasks. Keep agents streaming for load; never point it at prod.
//   node scripts/dev-perf/sidebarClicks.mjs [--port 9223] [--url-match ORIGIN] [--clicks 12] [--jitter 0]
//     [--hold 90] [--idle SECONDS] [--json]
// --idle only counts /api/mux/state fetches for that many seconds without input.
// --jitter moves the pointer that many pixels between press and release, as a trackpad click does.
import { connectPage, sleep } from "./cdp.mjs";

const args = process.argv.slice(2);
const option = (name, fallback) => { const index = args.indexOf(name); return index >= 0 ? args[index + 1] : fallback; };
const clicks = Number(option("--clicks", 12));
const jitter = Number(option("--jitter", 0));
const hold = Number(option("--hold", 90));
const page = await connectPage(option("--port") ? Number(option("--port")) : undefined, option("--url-match"));

let stateFetches = 0;
page.on("Network.requestWillBeSent", ({ request }) => { if (request.url.includes("/api/mux/state")) stateFetches++; });
await page.call("Network.enable");
const runStarted = performance.now();
const idle = Number(option("--idle", 0));
if (idle) {
  await sleep(idle * 1000);
  console.log(JSON.stringify({ idleSeconds: idle, stateFetches, statePerFiveSeconds: Math.round((stateFetches / idle) * 5 * 10) / 10 }));
  page.close();
  process.exit(0);
}

const mouse = (type, x, y, extra = {}) => page.call("Input.dispatchMouseEvent", { type, x, y, button: "none", ...extra });
const frame = () => page.evaluate("new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)))");

const SCROLLER = `document.querySelector(".ct-sidebar-content")`;
const ACTIVE = `document.querySelector(".ct-sidebar-content [data-tab-id] button[aria-current=page]")?.closest("[data-tab-id]")?.dataset.tabId ?? null`;

await page.evaluate(`(() => {
  const scroller = ${SCROLLER};
  if (!scroller) throw new Error("sidebar is not open");
  if (window.__sidebarClicks) return;
  const probe = window.__sidebarClicks = { longTasks: [], drags: 0, transformed: new Set(), scrolls: [] };
  new PerformanceObserver((list) => { for (const entry of list.getEntries()) probe.longTasks.push(entry.duration); })
    .observe({ type: "longtask", buffered: false });
  new MutationObserver((records) => {
    for (const record of records) {
      const node = record.target;
      if (record.attributeName === "data-dnd-dragging" && node.getAttribute("data-dnd-dragging") === "true") probe.drags++;
      const offsets = node.style.transform.match(/-?[\\d.]+px/g) ?? [];
      if (record.attributeName === "style" && offsets.some((offset) => parseFloat(offset) !== 0)) probe.transformed.add(node);
    }
  }).observe(scroller, { subtree: true, attributes: true, attributeFilter: ["data-dnd-dragging", "style"] });
  scroller.addEventListener("scroll", () => probe.scrolls.push(scroller.scrollTop), { passive: true });
})()`);

const scrollAway = await page.evaluate(`(() => {
  const scroller = ${SCROLLER};
  const max = scroller.scrollHeight - scroller.clientHeight;
  if (max < 40) throw new Error("sidebar content does not scroll; open a workspace with more tabs");
  if (scroller.scrollTop < Math.min(200, max / 2)) scroller.scrollTop = Math.round(max / 2);
  return { scrollTop: Math.round(scroller.scrollTop), max };
})()`);
await frame();

const VIEW = `(() => {
  const scroller = ${SCROLLER};
  const bounds = scroller.getBoundingClientRect();
  const rows = {};
  for (const row of scroller.querySelectorAll("[data-tab-id]")) {
    const r = row.getBoundingClientRect();
    if (r.height > 0 && r.top >= bounds.top && r.bottom <= bounds.bottom) rows[row.dataset.tabId] = { top: r.top, x: r.left + Math.min(r.width / 2, 80), y: r.top + r.height / 2 };
  }
  return { scrollTop: scroller.scrollTop, top: bounds.top, bottom: bounds.bottom, rows, active: ${ACTIVE} };
})()`;

const results = [];
let rotation = 0;
for (let index = 0; index < clicks; index++) {
  const before = await page.evaluate(VIEW);
  // Away from the edges, where a drag would also auto-scroll the list.
  const candidates = Object.entries(before.rows)
    .filter(([id, row]) => id !== before.active && row.y > before.top + 60 && row.y < before.bottom - 60);
  if (!candidates.length) throw new Error("no clickable row away from the sidebar edges");
  const [target, row] = candidates[rotation++ % candidates.length];
  await page.evaluate(`(() => { const p = window.__sidebarClicks; p.longTasks = []; p.drags = 0; p.transformed = new Set(); p.scrolls = []; })()`);
  const started = performance.now();
  await mouse("mouseMoved", row.x, row.y);
  await mouse("mousePressed", row.x, row.y, { button: "left", buttons: 1, clickCount: 1 });
  const steps = Math.max(1, Math.ceil(Math.abs(jitter) / 2));
  for (let step = 1; jitter && step <= steps; step++) {
    await mouse("mouseMoved", row.x, row.y + (jitter * step) / steps, { buttons: 1 });
  }
  await sleep(hold);
  await mouse("mouseReleased", row.x, row.y + jitter, { button: "left", buttons: 0, clickCount: 1 });
  await frame();
  const immediately = await page.evaluate(VIEW);
  let after = immediately;
  while (after.active !== target && performance.now() - started < 2000) {
    await frame();
    after = await page.evaluate(VIEW);
  }
  const shifts = Object.entries(before.rows)
    .filter(([id]) => immediately.rows[id])
    .map(([id, previous]) => Math.abs(immediately.rows[id].top - previous.top));
  const probe = await page.evaluate(`(() => { const p = window.__sidebarClicks; return { longTasks: p.longTasks, drags: p.drags, transformed: p.transformed.size, scrolls: p.scrolls }; })()`);
  const result = {
    target,
    activated: after.active,
    correct: after.active === target,
    activationMs: Math.round(performance.now() - started),
    scrollJump: Math.round(Math.max(0, ...probe.scrolls.map((top) => Math.abs(top - before.scrollTop)), Math.abs(after.scrollTop - before.scrollTop))),
    maxRowShift: Math.round(Math.max(0, ...shifts)),
    drags: probe.drags,
    transformedRows: probe.transformed,
    longTasks: probe.longTasks.filter((duration) => duration > 50).length,
    longestTask: Math.round(Math.max(0, ...probe.longTasks)),
  };
  results.push(result);
  if (!args.includes("--json")) console.log(JSON.stringify(result));
  await sleep(400);
}

const summary = {
  clicks: results.length,
  jitter,
  hold,
  startScroll: scrollAway,
  wrongTarget: results.filter((result) => !result.correct).length,
  scrollJumps: results.filter((result) => result.scrollJump > 2).length,
  maxScrollJump: Math.max(0, ...results.map((result) => result.scrollJump)),
  rowShifts: results.filter((result) => result.maxRowShift > 2).length,
  maxRowShift: Math.max(0, ...results.map((result) => result.maxRowShift)),
  dragStarts: results.reduce((sum, result) => sum + result.drags, 0),
  transformedClicks: results.filter((result) => result.transformedRows > 0).length,
  longTasks: results.reduce((sum, result) => sum + result.longTasks, 0),
  longestTask: Math.max(0, ...results.map((result) => result.longestTask)),
  medianActivationMs: results.map((result) => result.activationMs).sort((a, b) => a - b)[results.length >> 1],
  stateFetches,
  statePerFiveSeconds: Math.round((stateFetches / ((performance.now() - runStarted) / 1000)) * 5 * 10) / 10,
};
console.log(JSON.stringify(args.includes("--json") ? { summary, results } : summary));
page.close();
