import assert from "node:assert/strict";
import { Window } from "happy-dom";
import type { ApiConfig } from "../../lib/types";

async function main() {
  const win = new Window({ url: "http://localhost:7686" });
  Object.assign(globalThis, { window: win, localStorage: win.localStorage, sessionStorage: win.sessionStorage });
  const { setTokenFor } = await import("../../lib/webApi");
  const { fetchPlanControlLoop, setPlanControlLoop } = await import("./plansApi");
  const api: ApiConfig = { enabled: true, host: "localhost", port: 7686, token: "fixture", cors: true };
  setTokenFor("http://localhost:7686", "fixture");
  const calls: Array<{ url: string; init?: RequestInit }> = [];
  const stored = { mode: "heartbeat", heartbeat_interval_secs: 1800, next_at: null, last_round: null, fallback_reason: null, suspended_reason: "paused", counts_7d: { rounds: 0, quiet: 0, nudged: 0, learnings: 0, saves: 0, fallbacks: 0 } };
  globalThis.fetch = async (input, init) => {
    calls.push({ url: String(input), init });
    return Response.json(stored);
  };
  assert.deepEqual(await fetchPlanControlLoop(api, "plan/one"), stored);
  assert.equal(calls[0].url, "http://localhost:7686/api/plans/plan%2Fone/control-loop");
  assert.deepEqual(await setPlanControlLoop(api, "plan/one", { mode: "heartbeat", heartbeat_interval_secs: 1800 }), stored);
  assert.equal(calls[1].init?.method, "POST");
  assert.equal(new Headers(calls[1].init?.headers).get("X-CodeTerm-Plan-Confirmation"), "plan/one");
  assert.deepEqual(JSON.parse(String(calls[1].init?.body)), { mode: "heartbeat", heartbeat_interval_secs: 1800 });
  await setPlanControlLoop(api, "plan/one", { mode: "curator" });
  assert.deepEqual(JSON.parse(String(calls[2].init?.body)), { mode: "curator" });
  globalThis.fetch = async () => new Response("refused-fixture", { status: 403 });
  await assert.rejects(setPlanControlLoop(api, "plan/one", { mode: "curator" }), /403/);
  await assert.rejects(fetchPlanControlLoop(api, "plan/one"), /403/);
  console.log("plansApi control-loop: 4 cases passed");
}

void main().catch((error: unknown) => { console.error(error); process.exitCode = 1; });
