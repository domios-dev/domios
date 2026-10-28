// Caller: build.sh chat-scroll-trace; skill dev-testing
// Drives a touch scroll up the harness at iPhone size over CDP and prints a summary; exits 1 on any visual jump.
// Usage: node trace.mjs --cdp <browser ws> --url <harness url> [--out DIR] [--strokes N] [--threshold PX] [--jump]
import { sameAnchorLayoutResidual } from "../chat-scroll-residual.mjs";
import { mkdirSync, writeFileSync } from "node:fs";
import path from "node:path";
import { parseArgs } from "node:util";

const { values } = parseArgs({ options: {
  cdp: { type: "string" }, url: { type: "string" }, out: { type: "string" }, target: { type: "string" },
  strokes: { type: "string", default: "14" }, threshold: { type: "string", default: "8" }, jump: { type: "boolean", default: false },
  scenario: { type: "string", default: "touch" },
} });
// "touch" drives finger strokes; "grow" grows and collapses rows like tool cards expanding, pinned and scrolled up.
const growScenario = values.scenario === "grow";
if (!values.cdp || !values.url) {
  console.error("usage: trace.mjs --cdp <browser ws> --url <harness url> [--out DIR] [--strokes N] [--threshold PX] [--jump]");
  process.exit(2);
}
const strokes = Number(values.strokes);
const limit = Number(values.threshold);
const VIEWPORT = { width: 390, height: 844 };
const FINGER_STEP = 20;
const STEPS_PER_STROKE = 30;

const targets = await (await fetch(`http://127.0.0.1:${new URL(values.cdp).port}/json/list`)).json();
const page = targets.find((target) => values.target ? target.id === values.target : target.type === "page");
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
await send("Page.addScriptToEvaluateOnNewDocument", { source: `
  window.__scrollWrites = [];
  const descriptor = Object.getOwnPropertyDescriptor(Element.prototype, "scrollTop");
  Object.defineProperty(Element.prototype, "scrollTop", { ...descriptor, set(value) {
    const before = descriptor.get.call(this);
    descriptor.set.call(this, value);
    const after = descriptor.get.call(this);
    window.__scrollWrites.push({ at: performance.now(), before, after, owned: window.chatScrollOwned?.() ?? false });
  } });
` });
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
      // The grow scenario keeps every mounted row, so a tall row across the top edge stays in view of the metric.
      if (${growScenario} || (top > -900 && top < el.clientHeight + 900)) rows[row.dataset.chatWindowRow] = top;
    }
    // Touch strokes track the first row below the top edge; the grow scenario tracks the row the reader is in at the top
    // edge, since growth inside a row the reader sees pushes only what lies below it.
    const ids = Object.keys(rows);
    const anchor = ${growScenario} ? (ids.filter(id => rows[id] <= 0).at(-1) ?? ids[0]) : (ids.find(id => rows[id] >= 0) ?? ids.at(-1));
    window.__frames.push({ anchor, anchorTop: rows[anchor], t: performance.now(), scrollTop: el.scrollTop, spacer: el.firstElementChild.getBoundingClientRect().height, gesture, rows,
      gap: el.scrollHeight - el.clientHeight - el.scrollTop });
    next();
  };
  // The grow scenario reads each frame after it painted, so a layout the frame corrects before paint is not a jump.
  const channel = new MessageChannel();
  channel.port1.onmessage = sample;
  const next = ${growScenario} ? () => requestAnimationFrame(() => channel.port2.postMessage(0)) : () => requestAnimationFrame(sample);
  next();
  return true;
})()`);
await screenshot("start.png");

if (growScenario) {
  const waitFrames = (count) => evaluate(`new Promise((resolve) => { let seen = 0; const tick = () => ++seen >= ${count} ? resolve(true) : requestAnimationFrame(tick); requestAnimationFrame(tick); })`);
  const frameCount = () => evaluate("window.__frames.length");
  // Picks a mounted row by where it sits against the viewport and grows it; returns its index for the collapse.
  const growRow = (pick, px, timing) => evaluate(`(() => {
    const el = document.getElementById("scroller");
    const origin = el.getBoundingClientRect().top;
    const rows = [...el.querySelectorAll("[data-chat-window-row]")].map((row) => {
      const box = row.getBoundingClientRect();
      return { index: Number(row.dataset.chatWindowRow.slice(1)), top: box.top - origin, bottom: box.bottom - origin };
    });
    const row = (${pick})(rows, el.clientHeight);
    if (!row) return null;
    window.chatGrow({ index: row.index, px: ${px}, timing: "${timing}" });
    return row.index;
  })()`);
  const resize = (index, px, timing) => evaluate(`(window.chatGrow({ index: ${index}, px: ${px}, timing: "${timing}" }), ${index})`);
  const phases = [];
  const phase = async (label, kind, act) => {
    const from = await frameCount();
    const index = await act();
    await waitFrames(10);
    phases.push({ label, kind, index, from, to: await frameCount() });
    return index;
  };
  for (const timing of ["commit", "effect"]) {
    const last = await phase(`pinned: last row grows (${timing})`, "pinned", () => growRow("(rows) => rows.at(-1)", 320, timing));
    await phase(`pinned: last row collapses (${timing})`, "pinned", () => resize(last, -320, timing));
  }
  // At the bottom with a finger on the glass, a command streams in; the view must not leap when the finger lifts.
  const touchAt = (type, y) => send("Input.dispatchTouchEvent", { type, touchPoints: type === "touchEnd" ? [] : [{ x: VIEWPORT.width / 2, y, id: 1 }] });
  await phase("pinned, finger resting: the last row grows", "detached", async () => { await touchAt("touchStart", 500); return growRow("(rows) => rows.at(-1)", 320, "effect"); });
  await phase("pinned: the finger lifts after that growth", "detached", async () => { await touchAt("touchEnd"); return null; });
  await phase("pinned, finger nudging down: the last row grows", "detached", async () => {
    await touchAt("touchStart", 400);
    for (let step = 0; step < 8; step++) {
      await touchAt("touchMove", 400 - step * 4);
      if (step === 3) await growRow("(rows) => rows.at(-1)", 320, "effect");
      await nextFrame();
    }
    return null;
  });
  await phase("pinned: the finger lifts after that nudge", "detached", async () => { await touchAt("touchEnd"); return null; });
  await evaluate(`(document.getElementById("scroller").scrollTop -= 3000, true)`);
  await waitFrames(20);
  for (const timing of ["commit", "effect"]) {
    const above = await phase(`scrolled up: a row above the view grows (${timing})`, "detached", () => growRow("(rows) => rows.filter((row) => row.bottom < 0).at(-1)", 420, timing));
    await phase(`scrolled up: that row collapses (${timing})`, "detached", () => resize(above, -420, timing));
    const straddle = await phase(`scrolled up: the row across the top edge grows (${timing})`, "detached", () => growRow("(rows) => rows.find((row) => row.top < 0 && row.bottom > 0)", 420, timing));
    await phase(`scrolled up: that row collapses (${timing})`, "detached", () => resize(straddle, -420, timing));
  }
  // A finger on the glass owns the gesture; a tap on a card's header is such a touch, and its click expands the card.
  const touch = (type) => send("Input.dispatchTouchEvent", { type, touchPoints: type === "touchEnd" ? [] : [{ x: VIEWPORT.width / 2, y: 400, id: 1 }] });
  const held = await phase("finger resting: a row above the view grows", "detached", async () => { await touch("touchStart"); return growRow("(rows) => rows.filter((row) => row.bottom < 0).at(-1)", 420, "commit"); });
  await phase("finger lifted after that growth", "detached", async () => { await touch("touchEnd"); return held; });
  await phase("after a tap: the row across the top edge grows", "detached", async () => { await touch("touchStart"); await touch("touchEnd");
    return growRow("(rows) => rows.find((row) => row.top < 0 && row.bottom > 0)", 420, "effect"); });
  await phase("after a tap: a row above the view grows", "detached", async () => { await touch("touchStart"); await touch("touchEnd");
    return growRow("(rows) => rows.filter((row) => row.bottom < 0).at(-1)", 420, "effect"); });
  // The owner's case: slightly scrolled up, the finger moving, while command messages arrive and grow.
  const stroke = async (direction, during) => {
    let y = direction > 0 ? 200 : 700;
    await touch("touchStart");
    await send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [{ x: VIEWPORT.width / 2, y, id: 1 }] });
    for (let step = 0; step < STEPS_PER_STROKE; step++) {
      y += direction * FINGER_STEP / 2;
      await send("Input.dispatchTouchEvent", { type: "touchMove", touchPoints: [{ x: VIEWPORT.width / 2, y, id: 1 }] });
      await during(step);
      await nextFrame();
    }
    await touch("touchEnd");
  };
  await evaluate(`(() => { const el = document.getElementById("scroller"); el.scrollTop = el.scrollHeight - el.clientHeight - 600; return true; })()`);
  await waitFrames(20);
  for (const direction of [1, -1]) {
    const way = direction > 0 ? "up the history" : "down";
    for (const timing of ["commit", "effect"]) {
      await phase(`finger moving ${way}: only a row above grows (${timing})`, "moving", async () => {
        await stroke(direction, async (step) => { if (step === 15) await growRow("(rows) => rows.filter((row) => row.bottom < 0).at(-1)", 420, timing); });
        return null;
      });
    }
    await phase(`finger moving ${way}: the last rows grow and a row above grows`, "moving", async () => {
      await stroke(direction, async (step) => {
        if (step % 6 === 2) await growRow("(rows) => rows.at(-1)", 140, "effect");
        if (step === 15) await growRow("(rows) => rows.filter((row) => row.bottom < 0).at(-1)", 420, "effect");
        if (step === 22) await growRow("(rows) => rows.find((row) => row.top < 0 && row.bottom > 0)", 300, "commit");
      });
      return null;
    });
  }
  await screenshot("end.png");
  const frames = await evaluate("new Promise((resolve) => requestAnimationFrame(() => { window.__sampling = false; resolve(window.__frames); }))");
  ws.close();
  const results = phases.map((entry) => {
    const span = frames.slice(Math.max(0, entry.from - 1), entry.to);
    if (entry.kind === "pinned") return { ...entry, maxBottomGap: Math.round(Math.max(...span.map((frame) => frame.gap))) };
    // While the finger moves, the view follows it; only a shift the scroll does not explain is a jump.
    if (entry.kind === "moving") {
      let maxLayoutJump = 0;
      for (let index = 1; index < span.length; index++) {
        const result = sameAnchorLayoutResidual(span[index - 1], span[index], limit, false);
        if (result) maxLayoutJump = Math.max(maxLayoutJump, Math.abs(result.residual));
      }
      return { ...entry, maxVisualJump: Math.round(maxLayoutJump) };
    }
    let maxVisualJump = 0;
    for (let index = 1; index < span.length; index++) {
      const result = sameAnchorLayoutResidual(span[index - 1], span[index], limit, true);
      if (result) maxVisualJump = Math.max(maxVisualJump, Math.abs(result.visualDelta));
    }
    return { ...entry, maxVisualJump: Math.round(maxVisualJump) };
  });
  const failed = results.filter((entry) => entry.kind === "pinned" ? entry.maxBottomGap > 2 : entry.maxVisualJump > limit);
  const summary = { url: values.url, scenario: "grow", frames: frames.length, phases: results.map(({ label, index, maxBottomGap, maxVisualJump }) => ({ label, row: index, maxBottomGap, maxVisualJump })), failed: failed.length };
  if (values.out) writeFileSync(path.join(values.out, "trace.json"), JSON.stringify({ summary, phases, frames }));
  console.log(JSON.stringify(summary, null, 1));
  process.exit(failed.length > 0 ? 1 : 0);
}

let syntheticClicks = 0;
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
  if (values.jump) syntheticClicks += Number(await evaluate(`(() => {
    if (!window.chatScrollOwned?.()) return false;
    const button = document.querySelector("#jump button");
    button.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
    button.dispatchEvent(new MouseEvent("click", { bubbles: true, detail: 1 }));
    return true;
  })()`));
}
await screenshot("end.png");
const frames = await evaluate("new Promise((resolve) => requestAnimationFrame(() => { window.__sampling = false; resolve(window.__frames); }))");
const writes = await evaluate("window.__scrollWrites");
ws.close();

const jumps = [];
let residualPairs = 0;
for (let index = 1; index < frames.length; index++) {
  const before = frames[index - 1], after = frames[index];
  const compensated = writes.some(write => write.at > before.t && write.at <= after.t);
  const result = sameAnchorLayoutResidual(before, after, limit, compensated);
  if (!result) continue;
  residualPairs++;
  if (result.failed) jumps.push({ frame: index, residual: result.compensated ? result.visualDelta : result.residual });
}
const summary = {
  url: values.url, frames: frames.length, gestures: frames.at(-1)?.gesture ?? 0,
  scrollTopFirst: Math.round(frames[0]?.scrollTop ?? 0), scrollTopLast: Math.round(frames.at(-1)?.scrollTop ?? 0),
  residualPairs, syntheticClicks,
  visualJumps: jumps.length, maxVisualJump: jumps.reduce((worst, jump) => Math.max(worst, Math.abs(jump.residual)), 0),
  firstJumps: jumps.slice(0, 5),
  writesWhileOwned: writes.filter(write => write.owned).length,
};
if (values.out) {
  writeFileSync(path.join(values.out, "trace.json"), JSON.stringify({ summary,
    writes, frames: frames.map(({ t, scrollTop, spacer, gesture, anchor, anchorTop }) => ({ t, scrollTop, spacer, gesture, anchor, anchorTop })) }));
}
console.log(JSON.stringify(summary, null, 1));
process.exit(jumps.length > 0 || residualPairs === 0 || (values.jump && syntheticClicks === 0) || summary.writesWhileOwned > 0 ? 1 : 0);
