// Caller: build.sh chat-scroll-trace; skill dev-testing
// Drives a touch scroll up the harness at iPhone size over CDP and prints a summary; exits 1 on any visual jump.
// Usage: node trace.mjs --cdp <browser ws> --url <harness url> [--out DIR] [--strokes N] [--threshold PX]
import { mkdirSync, writeFileSync } from "node:fs";
import path from "node:path";
import { parseArgs } from "node:util";

const { values } = parseArgs({ options: {
  cdp: { type: "string" }, url: { type: "string" }, out: { type: "string" },
  strokes: { type: "string", default: "14" }, threshold: { type: "string", default: "2" },
} });
if (!values.cdp || !values.url) {
  console.error("usage: trace.mjs --cdp <browser ws> --url <harness url> [--out DIR] [--strokes N] [--threshold PX]");
  process.exit(2);
}
const strokes = Number(values.strokes);
const limit = Number(values.threshold);
const VIEWPORT = { width: 390, height: 844 };
const FINGER_STEP = 20;
const STEPS_PER_STROKE = 30;

const targets = await (await fetch(`http://127.0.0.1:${new URL(values.cdp).port}/json/list`)).json();
const page = targets.find((target) => target.type === "page");
if (!page) throw new Error("the browser has no page target");
const ws = new WebSocket(page.webSocketDebuggerUrl);
await new Promise((resolve, reject) => {
  ws.addEventListener("open", resolve, { once: true });
  ws.addEventListener("error", reject, { once: true });
});
let nextId = 1;
const pending = new Map();
const listeners = new Map();
ws.addEventListener("message", (event) => {
  const message = JSON.parse(event.data);
  if (message.id && pending.has(message.id)) {
    const { resolve, reject } = pending.get(message.id);
    pending.delete(message.id);
    if (message.error) reject(new Error(JSON.stringify(message.error)));
    else resolve(message.result);
  } else if (message.method && listeners.has(message.method)) listeners.get(message.method)(message.params);
});
const send = (method, params = {}) => new Promise((resolve, reject) => {
  const id = nextId++;
  pending.set(id, { resolve, reject });
  ws.send(JSON.stringify({ id, method, params }));
});
const once = (method) => new Promise((resolve) => listeners.set(method, (params) => {
  listeners.delete(method);
  resolve(params);
}));
const evaluate = async (expression) => (await send("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true })).result.value;
const nextFrame = () => evaluate("new Promise((resolve) => requestAnimationFrame(() => resolve(true)))");
const screenshot = async (file) => {
  if (!values.out) return;
  const shot = await send("Page.captureScreenshot", { format: "png" });
  writeFileSync(path.join(values.out, file), Buffer.from(shot.data, "base64"));
};
if (values.out) mkdirSync(values.out, { recursive: true });

await send("Page.enable");
await send("Emulation.setDeviceMetricsOverride", { ...VIEWPORT, deviceScaleFactor: 3, mobile: true });
await send("Emulation.setTouchEmulationEnabled", { enabled: true, maxTouchPoints: 5 });
const loaded = once("Page.loadEventFired");
await send("Page.navigate", { url: values.url });
await loaded;

// Ready means the pinned tail sat at the bottom for ten consecutive frames.
await evaluate(`new Promise((resolve) => {
  let stable = 0;
  const tick = () => {
    const el = document.getElementById("scroller");
    const atBottom = el && el.scrollHeight > el.clientHeight && el.scrollHeight - el.clientHeight - el.scrollTop < 2;
    stable = atBottom ? stable + 1 : 0;
    if (stable >= 10) resolve(true); else requestAnimationFrame(tick);
  };
  requestAnimationFrame(tick);
})`);

await evaluate(`(() => {
  const el = document.getElementById("scroller");
  window.__frames = [];
  window.__sampling = true;
  let gesture = 0;
  el.addEventListener("touchstart", () => { gesture++; }, { passive: true });
  const sample = () => {
    if (!window.__sampling) return;
    const origin = el.getBoundingClientRect().top;
    const rows = {};
    for (const row of el.querySelectorAll("[data-chat-window-row]")) {
      const top = row.getBoundingClientRect().top - origin;
      if (top > -900 && top < el.clientHeight + 900) rows[row.dataset.chatWindowRow] = top;
    }
    window.__frames.push({ t: performance.now(), scrollTop: el.scrollTop, spacer: el.firstElementChild.getBoundingClientRect().height, gesture, rows });
    requestAnimationFrame(sample);
  };
  requestAnimationFrame(sample);
  return true;
})()`);
await screenshot("start.png");

for (let stroke = 0; stroke < strokes; stroke++) {
  let y = 150;
  await send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [{ x: VIEWPORT.width / 2, y, id: 1 }] });
  for (let step = 0; step < STEPS_PER_STROKE; step++) {
    y += FINGER_STEP;
    await send("Input.dispatchTouchEvent", { type: "touchMove", touchPoints: [{ x: VIEWPORT.width / 2, y, id: 1 }] });
    await nextFrame();
  }
  await send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
  await nextFrame();
}
await screenshot("end.png");
const frames = await evaluate("new Promise((resolve) => requestAnimationFrame(() => { window.__sampling = false; resolve(window.__frames); }))");
ws.close();

// Visible motion is the user's scroll only when it mirrors the scrollTop change. A frame that also moved the spacer
// (a fold) shows no layout motion of its own, so it is judged against the momentum speed of its plain neighbours.
const delta = (index, key) => frames[index] && frames[index - 1] ? frames[index][key] - frames[index - 1][key] : 0;
let scrollTopReversals = 0;
let spacerScrollFrames = 0;
const jumps = [];
for (let index = 1; index < frames.length; index++) {
  const previous = frames[index - 1];
  const current = frames[index];
  const spacerMoved = Math.abs(delta(index, "spacer")) >= 0.5;
  if (spacerMoved && Math.abs(delta(index, "scrollTop")) >= 0.5) spacerScrollFrames++;
  else if (current.gesture === previous.gesture && delta(index, "scrollTop") > 0.5) scrollTopReversals++;
  const visible = Object.keys(current.rows).filter((id) => id in previous.rows && current.rows[id] >= 0 && current.rows[id] < VIEWPORT.height);
  if (!visible.length) continue;
  const motion = current.rows[visible[0]] - previous.rows[visible[0]];
  const unexplained = motion + delta(index, "scrollTop");
  const folded = spacerMoved && Math.abs(delta(index, "scrollTop")) >= 0.5;
  const momentum = Math.max(2 * FINGER_STEP, ...[-3, -2, -1, 1, 2, 3]
    .map((offset) => Math.abs(delta(index + offset, "spacer")) < 0.5 ? Math.abs(delta(index + offset, "scrollTop")) : 0));
  const jump = folded ? motion < -limit || motion > momentum + limit : Math.abs(motion) > limit && Math.abs(unexplained) > limit;
  if (jump) jumps.push({ frame: index, scrollTop: Math.round(current.scrollTop), motion: Math.round(motion), unexplained: Math.round(unexplained) });
}
const summary = {
  url: values.url, frames: frames.length, gestures: frames.at(-1)?.gesture ?? 0,
  scrollTopFirst: Math.round(frames[0]?.scrollTop ?? 0), scrollTopLast: Math.round(frames.at(-1)?.scrollTop ?? 0),
  scrollTopMonotonicWithinGestures: scrollTopReversals === 0, scrollTopReversals, spacerScrollFrames,
  visualJumps: jumps.length, maxVisualJump: jumps.reduce((worst, jump) => Math.max(worst, Math.abs(jump.motion), Math.abs(jump.unexplained)), 0),
  firstJumps: jumps.slice(0, 5),
};
if (values.out) {
  writeFileSync(path.join(values.out, "trace.json"), JSON.stringify({ summary,
    frames: frames.map(({ t, scrollTop, spacer, gesture }) => ({ t, scrollTop, spacer, gesture })) }));
}
console.log(JSON.stringify(summary, null, 1));
process.exit(jumps.length > 0 || scrollTopReversals > 0 ? 1 : 0);
