import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { parseArgs } from "node:util";
const require = createRequire(import.meta.url);
const { CHAT_READABLE_WIDTH_REM } = require("../../src/components/Terminal/chatLayoutTokens.ts");
const { chatGeometryVisible, selectChatGeometrySurface } = require("../../src/components/Terminal/chatGeometrySurface.ts");

const { values } = parseArgs({ options: { cdp: { type: "string" }, target: { type: "string" } } });
if (!values.cdp) throw new Error("Pass --cdp <Domios Browser endpoint>; open DEV Chat with user and assistant messages first");
const api = new URL(process.env.CODETERM_API_URL);
assert.equal(api.port, "7686", "geometry eval targets default DEV only");
const health = await (await fetch(new URL("/api/health", api), { signal: AbortSignal.timeout(10_000) })).json();
const vitePort = health.instance?.vite_port;
assert(vitePort, "DEV health must expose its frontend port");
const endpoint = new URL(values.cdp);
assert(["127.0.0.1", "localhost"].includes(endpoint.hostname), "Browser endpoint must be local");
const targets = await (await fetch(`http://${endpoint.host}/json/list`, { signal: AbortSignal.timeout(10_000) })).json();
const page = targets.find((entry) => {
  if (entry.type !== "page" || (values.target && entry.id !== values.target)) return false;
  const url = new URL(entry.url);
  return ["127.0.0.1", "localhost"].includes(url.hostname) && url.port === String(vitePort);
});
assert(page, "Browser must contain a DEV frontend target; no fallback to unrelated pages");
const socket = new WebSocket(page.webSocketDebuggerUrl);
await new Promise((resolve, reject) => {
  socket.addEventListener("open", resolve, { once: true });
  socket.addEventListener("error", reject, { once: true });
});
let nextId = 0;
const pending = new Map();
socket.addEventListener("message", ({ data }) => {
  const message = JSON.parse(data);
  const waiter = pending.get(message.id);
  if (!waiter) return;
  pending.delete(message.id);
  if (message.error) waiter.reject(new Error(JSON.stringify(message.error)));
  else waiter.resolve(message.result);
});
const deadline = AbortSignal.timeout(60_000);
deadline.addEventListener("abort", () => {
  for (const waiter of pending.values()) waiter.reject(new Error("geometry CDP deadline"));
  socket.close();
});
const call = (method, params = {}) => new Promise((resolve, reject) => {
  if (deadline.aborted) return reject(new Error("geometry CDP deadline"));
  const id = ++nextId;
  pending.set(id, { resolve, reject });
  socket.send(JSON.stringify({ id, method, params }));
});
const evaluate = async (expression) => {
  const result = await call("Runtime.evaluate", { expression, returnByValue: true, awaitPromise: true });
  if (result.exceptionDetails) throw new Error(result.exceptionDetails.exception?.description ?? result.exceptionDetails.text);
  return result.result.value;
};

function measure(selectSurface, isVisible) {
  const visible = (element) => {
    const rect = element.getBoundingClientRect();
    const ancestors = [];
    for (let ancestor = element; ancestor; ancestor = ancestor.parentElement) {
      const style = getComputedStyle(ancestor);
      ancestors.push({
        hidden: ancestor.hidden, inert: ancestor.inert, ariaHidden: ancestor.getAttribute("aria-hidden") === "true",
        display: style.display, visibility: style.visibility, opacity: Number(style.opacity),
      });
    }
    return isVisible(rect.width, rect.height, ancestors);
  };
  const surfaces = [...document.querySelectorAll(".ct-chat-surface")];
  const candidates = surfaces.map((element) => {
    const shell = element.closest('[data-testid="tab-session-chat"]');
    return {
      tabId: shell?.dataset.tabId ?? null,
      visible: visible(element),
      snapshotReady: shell?.dataset.snapshotReady === "true",
      busy: shell?.getAttribute("aria-busy") === "true",
    };
  });
  const index = selectSurface(candidates);
  if (index === null) throw new Error(`expected one active ready transcript surface: ${JSON.stringify(candidates)}`);
  const surface = surfaces[index];
  const shell = surface.closest('[data-testid="tab-session-chat"]');
  if (shell?.dataset.snapshotReady !== "true" || shell.getAttribute("aria-busy") === "true") {
    throw new Error("transcript snapshot is not ready");
  }
  const box = (element) => {
    const rect = element.getBoundingClientRect();
    return { left: rect.left, width: rect.width, center: rect.left + rect.width / 2 };
  };
  const style = getComputedStyle(surface);
  const rect = surface.getBoundingClientRect();
  const left = rect.left + surface.clientLeft + parseFloat(style.paddingLeft);
  const width = surface.clientWidth - parseFloat(style.paddingLeft) - parseFloat(style.paddingRight);
  const rows = [...surface.querySelectorAll(".ct-chat-message-row")].filter(visible);
  return {
    viewport: innerWidth,
    tabId: shell.dataset.tabId,
    available: { left, width, center: left + width / 2 },
    rootFont: parseFloat(getComputedStyle(document.documentElement).fontSize),
    rows: rows.map(box),
    bubbles: [...surface.querySelectorAll('[role="group"][aria-label="User message"]')].filter(visible).map(box),
    assistants: [...surface.querySelectorAll(".ct-chat-message-row--agent .ct-chat-message-column")].filter(visible).map(box),
    composers: [...shell.querySelectorAll(".ct-composer-lane")].filter(visible).map(box),
  };
}

function check(sample, width) {
  assert.equal(sample.viewport, width, "viewport override took effect");
  const max = CHAT_READABLE_WIDTH_REM * sample.rootFont;
  const expected = Math.min(max, sample.available.width);
  assert(sample.rows.length && sample.bubbles.length && sample.assistants.length && sample.composers.length,
    "loaded transcript must include rows, user bubbles, assistant text and composer");
  if (width >= 1440) assert(sample.available.width > max, "wide scenario must exercise the max width");
  for (const box of [...sample.rows, ...sample.bubbles, ...sample.assistants]) {
    assert(Math.abs(box.width - expected) <= 2, `width ${box.width} differs from readable width ${expected}`);
    assert(Math.abs(box.center - sample.available.center) <= 2, `off-center by ${box.center - sample.available.center}px`);
  }
  for (const box of sample.composers) assert(box.width <= max + 2, "composer exceeds shared readable width");
}

try {
  for (const width of [1440, 1920, 2560, 390]) {
    await call("Emulation.setDeviceMetricsOverride", { width, height: 1000, deviceScaleFactor: 1, mobile: false });
    await evaluate("new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve(true))))");
    const sample = await evaluate(`(${measure.toString()})(${selectChatGeometrySurface.toString()}, ${chatGeometryVisible.toString()})`);
    console.log(JSON.stringify({ build: health.build, width, ...sample }));
    check(sample, width);
    const regressed = structuredClone(sample);
    regressed.rows[0].width = sample.available.width + 100;
    assert.throws(() => check(regressed, width), "oracle must reject an uncapped row");
    const displaced = structuredClone(sample);
    displaced.bubbles[0].center += 20;
    assert.throws(() => check(displaced, width), "oracle must reject an off-center bubble");
    console.log(`PASS ${width}: geometry and negative control`);
  }
} finally {
  await call("Emulation.clearDeviceMetricsOverride");
  socket.close();
}
