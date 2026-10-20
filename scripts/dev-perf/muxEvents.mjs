// Counts DEV `mux-state-changed` frames against the distinct published snapshots they announce
// (generation excluded). Keep agents streaming for load; never point it at prod.
//   node scripts/dev-perf/muxEvents.mjs [--seconds 10] [--api 7686] [--json]
import { devApiToken } from "./cdp.mjs";

const args = process.argv.slice(2);
const option = (name) => { const index = args.indexOf(name); return index >= 0 ? args[index + 1] : undefined; };
const seconds = Number(option("--seconds") ?? 10);
const api = option("--api") ?? "7686";
if (api === "7685") throw new Error("7685 is prod");
const base = `http://127.0.0.1:${api}`;
const headers = { Authorization: `Bearer ${devApiToken()}` };

const emitStats = async () => (await fetch(`${base}/api/debug/events/stats`, { headers })).json();
const contentOf = (body) => { const { generation, ...content } = JSON.parse(body); return { generation, content: JSON.stringify(content) }; };

const counts = { frames: 0, bare: 0, announced: 0, fetches: 0, notModified: 0, bytes: 0, distinct: 0, sameContent: 0 };
let lastContent = null;
let etag = null;
let inFlight = null;
let pending = false;
// One fetch in flight and at most one queued, exactly like the client, so the sample costs what a client costs.
const refetch = () => {
  if (inFlight) { pending = true; return; }
  inFlight = (async () => {
    do {
      pending = false;
      counts.fetches++;
      const response = await fetch(`${base}/api/mux/state`, { headers: etag ? { ...headers, "If-None-Match": etag } : headers });
      if (response.status === 304) { counts.notModified++; continue; }
      etag = response.headers.get("etag");
      const body = await response.text();
      counts.bytes += body.length;
      const { content } = contentOf(body);
      if (content === lastContent) counts.sameContent++;
      else if (lastContent !== null) counts.distinct++;
      lastContent = content;
    } while (pending);
  })().finally(() => { inFlight = null; });
};

const before = await emitStats();
refetch();
await inFlight;
const controller = new AbortController();
const stream = await fetch(`${base}/api/events/stream`, { headers, signal: controller.signal });
if (!stream.ok) throw new Error(`event stream ${stream.status}`);
const started = performance.now();
setTimeout(() => controller.abort(), seconds * 1000);
const decoder = new TextDecoder();
let buffer = "";
try {
  for await (const chunk of stream.body) {
    buffer += decoder.decode(chunk, { stream: true });
    let end;
    while ((end = buffer.indexOf("\n\n")) >= 0) {
      const block = buffer.slice(0, end);
      buffer = buffer.slice(end + 2);
      const data = block.split("\n").filter((line) => line.startsWith("data:")).map((line) => line.slice(5).trim()).join("\n");
      if (!data) continue;
      let frame;
      try { frame = JSON.parse(data); } catch { continue; }
      if (frame.event_type !== "mux-state-changed") continue;
      counts.frames++;
      if (frame.data?.generation === undefined) counts.bare++;
      else counts.announced++;
      refetch();
    }
  }
} catch (error) {
  if (error.name !== "AbortError") throw error;
}
const elapsed = (performance.now() - started) / 1000;
await inFlight;
const after = await emitStats();
const delta = (name) => (after[name] ?? 0) - (before[name] ?? 0);
const perSecond = (value) => Math.round((value / elapsed) * 10) / 10;
const result = {
  seconds: Math.round(elapsed * 10) / 10,
  framesPerSecond: perSecond(counts.frames),
  barePerSecond: perSecond(counts.bare),
  announcedPerSecond: perSecond(counts.announced),
  fetchesPerSecond: perSecond(counts.fetches),
  kilobytesPerSecond: Math.round(counts.bytes / 1024 / elapsed),
  distinctChanges: counts.distinct,
  sameContentFetches: counts.sameContent,
  notModified: counts.notModified,
  daemon: { requests: delta("emit_requests"), fires: delta("emit_fires"), suppressed: delta("emit_suppressed"), unchanged: delta("emit_unchanged") },
  frames: counts.frames,
};
if (args.includes("--json")) console.log(JSON.stringify(result));
else console.log(result);
