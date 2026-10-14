#!/usr/bin/env node
// Caller: skill dev-testing
// Real CDP mouse-wheel at a selector's center; reports which scroller moved. No Playwright, no deps.
// Usage: node scripts/dev-wheel-probe.mjs [css-selector ...] [--api-port 7686] [--delta 240]
// With no selector it probes a sidebar tab label, the same row's icon, and empty sidebar space.
const args = process.argv.slice(2);
const flag = (name, fallback) => {
  const i = args.indexOf(name);
  return i >= 0 ? args.splice(i, 2)[1] : fallback;
};
const apiPort = flag("--api-port", "7686");
const deltaY = Number(flag("--delta", "240"));
const selectors = args;

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
const call = (method, params = {}) => new Promise((resolve, reject) => {
  pending.set(++nextId, { resolve, reject });
  ws.send(JSON.stringify({ id: nextId, method, params }));
});
ws.onmessage = (event) => {
  const message = JSON.parse(event.data);
  const waiter = pending.get(message.id);
  if (!waiter) return;
  pending.delete(message.id);
  if (message.error) waiter.reject(new Error(JSON.stringify(message.error)));
  else waiter.resolve(message.result);
};
await new Promise((resolve) => { ws.onopen = resolve; });
const evaluate = async (expression, awaitPromise = false) => {
  const { result, exceptionDetails } = await call("Runtime.evaluate", { expression, returnByValue: true, awaitPromise });
  if (exceptionDetails) throw new Error(exceptionDetails.exception?.description ?? exceptionDetails.text);
  return result.value;
};

const PAGE_HELPERS = `
(() => {
  if (window.__wheelProbe) return;
  const visible = (e) => e.getClientRects().length && e.getBoundingClientRect().width;
  const scrollable = (el) => {
    const s = getComputedStyle(el);
    return /(auto|scroll)/.test(s.overflowY) && el.scrollHeight > el.clientHeight;
  };
  const nearestScroller = (el) => {
    for (let n = el; n; n = n.parentElement) if (scrollable(n)) return n;
    return null;
  };
  const describe = (n) => n.tagName.toLowerCase()
    + (n.getAttribute("data-testid") ? "[" + n.getAttribute("data-testid") + "]" : "")
    + (typeof n.className === "string" && n.className ? "." + n.className.trim().split(/\\s+/).slice(0, 3).join(".") : "");
  const chain = (el) => {
    const out = [];
    for (let n = el; n && n !== document.documentElement; n = n.parentElement) {
      const s = getComputedStyle(n);
      out.push({
        node: describe(n),
        overflow: s.overflowX + "/" + s.overflowY,
        overscroll: s.overscrollBehaviorY,
        pointerEvents: s.pointerEvents,
        appRegion: s.getPropertyValue("-webkit-app-region") || "none",
        touchAction: s.touchAction,
        scrollable: scrollable(n),
      });
    }
    return out;
  };
  const pick = (selector) => [...document.querySelectorAll(selector)].find(visible) ?? null;
  const center = (el) => { const r = el.getBoundingClientRect(); return [r.x + r.width / 2, r.y + r.height / 2]; };
  const emptySpace = () => {
    const scroller = pick(".ct-sidebar-content");
    if (!scroller) return null;
    const r = scroller.getBoundingClientRect();
    for (let y = r.bottom - 4; y > r.top; y -= 6) {
      const x = r.x + r.width - 6;
      if (document.elementFromPoint(x, y) === scroller) return [x, y];
    }
    return null;
  };
  window.__wheelProbe = { pick, center, nearestScroller, chain, emptySpace, describe };
})()`;
await evaluate(PAGE_HELPERS);

const resolvePoint = (spec) => evaluate(`(() => {
  const p = window.__wheelProbe;
  const spec = ${JSON.stringify(spec)};
  let el = null, point = null;
  if (spec.empty) { point = p.emptySpace(); el = point && document.elementFromPoint(point[0], point[1]); }
  else {
    const root = spec.within ? p.pick(spec.within)?.closest('[data-testid="tab-item"]') : document;
    el = root && (root === document ? p.pick(spec.selector) : root.querySelector(spec.selector));
    el?.scrollIntoView({ block: "nearest" });
    point = el && p.center(el);
  }
  if (!el || !point) return null;
  const hit = document.elementFromPoint(point[0], point[1]);
  const scroller = p.nearestScroller(hit);
  return { point, hit: p.describe(hit), chain: p.chain(hit), scroller: scroller && p.describe(scroller), scrollTop: scroller ? scroller.scrollTop : null };
})()`);

const arm = () => evaluate(`(() => {
  const p = window.__wheelProbe;
  window.__wheelResult = new Promise((resolve) => {
    let wheel = null;
    const done = (reason) => { removeEventListener("wheel", onWheel, true); removeEventListener("scroll", onScroll, true); resolve({ reason, wheel }); };
    const onWheel = (e) => {
      wheel = { target: p.describe(e.target), defaultPrevented: e.defaultPrevented, deltaY: e.deltaY };
      requestAnimationFrame(() => requestAnimationFrame(() => done("settled")));
    };
    const onScroll = () => done("scroll");
    addEventListener("wheel", onWheel, { capture: true, passive: true });
    addEventListener("scroll", onScroll, true);
    requestAnimationFrame(() => requestAnimationFrame(() => { if (!wheel) done("no-wheel-event"); }));
  });
})()`);

const specs = selectors.length
  ? selectors.map((selector) => ({ name: selector, selector }))
  : [
    { name: "tab label", selector: '[data-testid="tab-title"]' },
    { name: "same row icon", selector: '[data-testid="tab-tab-icon"]', within: '[data-testid="tab-title"]' },
    { name: "empty sidebar space", empty: true },
  ];

const rows = [];
for (const spec of specs) {
  const found = await resolvePoint(spec);
  if (!found) { rows.push({ name: spec.name, error: "target not found or not visible" }); continue; }
  const [x, y] = found.point;
  await call("Input.dispatchMouseEvent", { type: "mouseMoved", x, y });
  await arm();
  await call("Input.dispatchMouseEvent", { type: "mouseWheel", x, y, deltaX: 0, deltaY });
  const outcome = await evaluate("window.__wheelResult", true);
  const after = await evaluate(`(() => {
    const p = window.__wheelProbe;
    const s = p.nearestScroller(document.elementFromPoint(${x}, ${y}));
    return s ? s.scrollTop : null;
  })()`);
  rows.push({
    name: spec.name,
    point: [Math.round(x), Math.round(y)],
    hit: found.hit,
    scroller: found.scroller,
    scrollTopBefore: found.scrollTop,
    scrollTopAfter: after,
    moved: found.scrollTop !== after,
    event: outcome,
    chain: found.chain,
  });
  // Undo so the next probe starts from the same scroll offset.
  await evaluate(`(() => { const s = window.__wheelProbe.nearestScroller(document.elementFromPoint(${x}, ${y})); if (s) s.scrollTop = ${found.scrollTop ?? 0}; })()`);
}
ws.close();
console.log(JSON.stringify({ build: health.build, url: page.url, deltaY, probes: rows }, null, 2));
const failed = rows.filter((r) => r.error || !r.moved);
if (failed.length) {
  console.error(`NOT SCROLLED: ${failed.map((r) => r.name).join(", ")} (short content or a swallowed wheel; compare the chain)`);
  process.exitCode = 1;
}
