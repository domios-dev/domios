import { mkdirSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";

const [endpoint, title, marker, foreignMarker, output = "terminal-switch-trace", viewport, ownedTarget] = process.argv.slice(2);
if (!endpoint || !title || !marker || !foreignMarker) {
  throw new Error("Usage: node evals/terminal-switch/trace.mjs <Browser CDP endpoint> <tab title> <own marker> <foreign marker> <output directory>");
}
const directory = resolve(output);
mkdirSync(directory, { recursive: true });
const ws = new WebSocket(endpoint);
const deadline = AbortSignal.timeout(30_000);
let nextId = 0;
const pending = new Map();
deadline.addEventListener("abort", () => {
  for (const waiter of pending.values()) waiter.reject(new Error("Terminal trace exceeded its failure deadline"));
  ws.close();
}, { once: true });
let sessionId;
let capture = false;
let frames = 0;
ws.onmessage = ({ data }) => {
  const message = JSON.parse(data);
  if (message.method === "Page.screencastFrame" && capture) {
    writeFileSync(resolve(directory, `frame-${String(frames++).padStart(3, "0")}.png`), Buffer.from(message.params.data, "base64"));
    void call("Page.screencastFrameAck", { sessionId: message.params.sessionId });
  }
  const waiter = pending.get(message.id);
  if (!waiter) return;
  pending.delete(message.id);
  if (message.error) waiter.reject(new Error(JSON.stringify(message.error)));
  else waiter.resolve(message.result);
};
const call = (method, params = {}, target = sessionId) => new Promise((resolve, reject) => {
  pending.set(++nextId, { resolve, reject });
  ws.send(JSON.stringify({ id: nextId, method, params, sessionId: target }));
});
await new Promise((resolve, reject) => { ws.onopen = resolve; ws.onerror = reject; });
try {
  const { targetInfos } = await call("Target.getTargets");
  const pages = targetInfos.filter(target => target.type === "page" && target.url.startsWith("http://127.0.0.1:7686/")
    && (!ownedTarget || target.targetId === ownedTarget));
  if (pages.length !== 1) throw new Error("Expected one owned Browser page; refusing to select another target");
  ({ sessionId } = await call("Target.attachToTarget", { targetId: pages[0].targetId, flatten: true }));
  if (viewport) {
    const [width, height] = viewport.split("x").map(Number);
    if (!(width > 0 && height > 0)) throw new Error("Viewport must be WIDTHxHEIGHT");
    await call("Emulation.setDeviceMetricsOverride", { width, height, mobile: width <= 600, deviceScaleFactor: 1 });
  }
  capture = true;
  await call("Page.startScreencast", { format: "png", everyNthFrame: 1 });
  if (process.env.TERMINAL_TRACE_PROFILE === "1") {
    await call("Profiler.enable");
    await call("Profiler.start");
  }
  const result = await call("Runtime.evaluate", {
    awaitPromise: true, returnByValue: true,
    expression: `(${traceSwitch.toString()})(${JSON.stringify(title)},${JSON.stringify(marker)},${JSON.stringify(foreignMarker)})`,
  });
  await call("Page.stopScreencast");
  if (process.env.TERMINAL_TRACE_PROFILE === "1") {
    const { profile } = await call("Profiler.stop");
    writeFileSync(resolve(directory, "cpu-profile.json"), JSON.stringify(profile));
  }
  capture = false;
  if (result.exceptionDetails) throw new Error(result.exceptionDetails.exception?.description ?? result.exceptionDetails.text);
  const trace = result.result.value;
  const correct = trace.samples.find(sample => sample.layers.some(layer => layer.painted && layer.own));
  const summary = {
    firstCorrectFrameMs: correct?.ms ?? null,
    foreignFrameSamples: trace.samples.filter(sample => sample.layers.some(layer => layer.painted && layer.foreign)).length,
    networkReplayBytes: trace.network.bytes,
    terminalWriteBytes: trace.writes,
    capturedFrames: frames,
  };
  writeFileSync(resolve(directory, "trace.json"), JSON.stringify({ summary, ...trace }, null, 2));
  console.log(JSON.stringify({ directory, ...summary }, null, 2));
} finally {
  if (sessionId && viewport) await call("Emulation.clearDeviceMetricsOverride");
  ws.close();
}

function traceSwitch(title, marker, foreignMarker) {
  return new Promise((resolve, reject) => {
    const button = [...document.querySelectorAll("button")].find(button => button.getAttribute("aria-label") === `Open tab shell, ${title}`);
    if (!button) { reject(new Error(`Tab button not found: ${title}`)); return; }
    const start = performance.now();
    window.__outputStreamPerf = { frames: 0, bytes: 0, batches: 0, lastFrameAt: 0 };
    const samples = [], writes = {}, originals = new Map(), stages = [], longTasks = [];
    const stamp = () => performance.now() - start;
    const longTaskObserver = PerformanceObserver.supportedEntryTypes.includes("longtask")
      ? new PerformanceObserver(list => {
        for (const entry of list.getEntries()) {
          longTasks.push({ ms: entry.startTime - start, duration: entry.duration });
        }
      }) : null;
    longTaskObserver?.observe({ type: "longtask" });
    const hosts = () => [...document.querySelectorAll(".terminal-xterm-host")];
    const captureWrites = host => {
      const tab = host.closest('[data-testid="terminal-tab"]')?.dataset.tabId;
      const term = host.__codetermTerminal;
      if (!term || originals.has(term)) return;
      const write = term.write, resize = term.resize;
      stages.push({ stage: "terminal-discovered", tab, ms: stamp(), cols: term.cols, rows: term.rows });
      originals.set(term, { write, resize });
      term.write = function(data, done) {
        const bytes = typeof data === "string" ? new TextEncoder().encode(data).length : data.length;
        writes[tab] = (writes[tab] ?? 0) + bytes;
        const submitted = stamp();
        stages.push({ stage: "write-submitted", tab, ms: submitted, bytes });
        return write.call(this, data, () => {
          stages.push({ stage: "write-completed", tab, ms: stamp(), submitted, bytes });
          done?.();
        });
      };
      term.resize = function(cols, rows) {
        const submitted = stamp();
        const result = resize.call(this, cols, rows);
        stages.push({ stage: "resize", tab, ms: submitted, duration: stamp() - submitted, cols, rows });
        return result;
      };
    };
    const observer = new MutationObserver(() => hosts().forEach(captureWrites));
    observer.observe(document.body, { childList: true, subtree: true });
    hosts().forEach(captureWrites);
    const sample = () => {
      const layers = hosts().map(host => {
        const tab = host.closest('[data-testid="terminal-tab"]')?.dataset.tabId;
        const term = host.__codetermTerminal;
        captureWrites(host);
        let alpha = 1, visible = true;
        const scrolledAncestors = [];
        for (let node = host; node && node !== document.body; node = node.parentElement) {
          const style = getComputedStyle(node);
          alpha *= Number(style.opacity);
          visible &&= style.display !== "none" && style.visibility !== "hidden";
          if (node.scrollLeft) scrolledAncestors.push({ tag: node.tagName, className: node.className, scrollLeft: node.scrollLeft });
        }
        const buffer = term?.buffer.active;
        const text = buffer ? Array.from({ length: term.rows }, (_, i) => buffer.getLine(buffer.viewportY + i)?.translateToString(true)).join("\n") : "";
        const bounds = element => {
          const rect = element?.getBoundingClientRect();
          return rect ? { x: rect.x, y: rect.y, width: rect.width, height: rect.height } : null;
        };
        return {
          tab, alpha, painted: visible && alpha > 0 && host.getBoundingClientRect().width > 0,
          own: text.includes(marker), foreign: text.includes(foreignMarker),
          cols: term?.cols, rows: term?.rows, host: bounds(host),
          screen: bounds(host.querySelector(".xterm-screen")), scrolledAncestors,
        };
      });
      samples.push({ ms: performance.now() - start, layers });
      if (samples.length < 40) requestAnimationFrame(sample);
      else {
        observer.disconnect();
        longTaskObserver?.disconnect();
        for (const [term, original] of originals) Object.assign(term, original);
        resolve({ samples, writes, stages, longTasks, network: window.__outputStreamPerf });
      }
    };
    button.click();
    requestAnimationFrame(sample);
  });
}
