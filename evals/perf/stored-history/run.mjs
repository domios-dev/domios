import fs from "node:fs";
import path from "node:path";

const [tab, session, build, offset, count = "40", output = "target-check/evals/stored-history.json"] = process.argv.slice(2);
if (!process.env.CODETERM_DEV_DATA_DIR || !process.env.CODETERM_API_TOKEN) throw Error("Use the shared DEV wrapper");
const base = process.env.CODETERM_API_URL;
if (!/^http:\/\/127\.0\.0\.1:\d+$/.test(base ?? "") || new URL(base).port === "7685") throw Error("DEV endpoint required");
if (!tab || !session || !/^[a-f0-9]{40}$/.test(build ?? "")) throw Error("Expected tab, session, exact build SHA, before offset, page count, output");
let before = Number(offset);
const pages = Number(count);
if (!Number.isSafeInteger(before) || before <= 0 || !Number.isSafeInteger(pages) || pages < 20 || pages > 100) throw Error("Positive byte offset and 20..100 pages required");
const headers = { Authorization: `Bearer ${process.env.CODETERM_API_TOKEN}`, "Content-Type": "application/json" };
const request = async (route, body) => {
  const response = await fetch(`${base}${route}`, { headers, ...(body ? { method: "POST", body: JSON.stringify(body) } : {}), signal: AbortSignal.timeout(15000) });
  if (!response.ok) throw Error(`HTTP ${response.status}: ${route}`);
  return response;
};
const health = await (await request("/api/health")).json();
if (health.build_sha !== build) throw Error(`Build drift: ${health.build_sha}`);
const result = { build, tab, session, status: "incomplete", samples: [] };
let bindingNonce;
fs.mkdirSync(path.dirname(output), { recursive: true });
const save = () => fs.writeFileSync(output, JSON.stringify(result, null, 2));
try {
  for (let page = 0; page < pages && before > 0; page++) {
    const started = performance.now();
    const response = await request(`/api/tabs/${encodeURIComponent(tab)}/chat-raw/backfill`, { before_offset: before, offset_kind: "bytes" });
    const text = await response.text();
    const received = performance.now();
    const value = JSON.parse(text);
    if (value.source_session_id !== session || value.offset_kind !== "bytes") throw Error("History authority or cursor kind changed");
    if (page === 0) bindingNonce = value.session_binding_nonce;
    if (value.session_binding_nonce !== bindingNonce) throw Error("History binding changed");
    if (!(value.start_offset >= 0 && value.start_offset < value.end_offset && value.end_offset <= before)) throw Error("Cursor did not progress");
    if (!Array.isArray(value.messages)) throw Error("Missing projected rows");
    result.samples.push({ page, before, start: value.start_offset, end: value.end_offset, rows: value.messages.length,
      responseBytes: Buffer.byteLength(text), requestMs: received - started, decodeMs: performance.now() - received, totalMs: performance.now() - started });
    before = value.start_offset;
    save();
  }
  if (result.samples.length < 20) throw Error("Fewer than 20 pages: insufficient p95 sample");
  const finalHealth = await (await request("/api/health")).json();
  if (finalHealth.build_sha !== build || finalHealth.boot_id !== health.boot_id) throw Error("DEV restarted during benchmark");
  const times = result.samples.map(sample => sample.totalMs).sort((a, b) => a - b);
  result.p95Ms = times[Math.ceil(times.length * 0.95) - 1];
  result.status = result.p95Ms < 300 ? "pass" : "fail";
  console.log(JSON.stringify({ status: result.status, pages: times.length, p95Ms: result.p95Ms, output }));
  if (result.status !== "pass") process.exitCode = 1;
} finally {
  save();
}
