import assert from "node:assert/strict";
import { performance } from "node:perf_hooks";

const endpoint = new URL(process.env.CODETERM_API_URL ?? "http://invalid");
assert.equal(process.env.CODETERM_INSTANCE, "ocdec", "Run under the bench lib's named ocdec DEV scope");
assert.equal(["7685", "7686"].includes(endpoint.port), false, "Default instances are forbidden");
const token = process.env.CODETERM_API_TOKEN;
assert.ok(token, "The bench lib must provide DEV authentication");
const query = process.argv[2];
assert.ok(query, "Pass a query matching at least one seeded named-DEV memory fixture");
const count = Number(process.argv[3] ?? 20);
assert.ok(Number.isInteger(count) && count >= 20 && count <= 100);

async function get(path) {
  const response = await fetch(new URL(path, endpoint), {
    headers: { authorization: `Bearer ${token}` }, signal: AbortSignal.timeout(15000),
  });
  assert.equal(response.ok, true, `HTTP ${response.status}`);
  return response.json();
}

const initial = await get("/api/supporter/status");
assert.equal(initial.adapter, "opencode", "Select OpenCode only in named DEV before this probe");
const latencies = [];
for (let index = 0; index < count; index += 1) {
  const started = performance.now();
  const result = await get(`/api/mem/search?q=${encodeURIComponent(query)}&limit=1&supporter=true`);
  latencies.push(performance.now() - started);
  assert.equal(result.supporter?.status, "applied", `Decision ${index} did not reach its applied result`);
  assert.equal(result.supporter?.adapter, "opencode");
}
const final = await get("/api/supporter/status");
const physicalCalls = final.calls_last_hour - initial.calls_last_hour;
assert.ok(physicalCalls >= count, "Cached or skipped results do not prove twenty physical decisions");
const sorted = [...latencies].sort((left, right) => left - right);
const percentile = (fraction) => sorted[Math.ceil(sorted.length * fraction) - 1];
const summary = {
  instance: "ocdec", backend: "run", samples: count, physicalCalls,
  metric: "end-to-end memory decision request (upper bound on decision latency)",
  deadlineMs: 5000, p50Ms: percentile(0.5), p95Ms: percentile(0.95), latenciesMs: latencies,
};
console.log(JSON.stringify(summary, null, 2));
assert.ok(summary.p95Ms <= summary.deadlineMs, "The measured p95 exceeds the five-second target");
