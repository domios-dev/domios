// WHAT: First-run access, completion and fixture session ownership.
// WHY: Setup must retain persisted choices and exact session authority.
// ORACLE: Injected API calls and readback show completion and binding once.
import assert from "node:assert/strict";
import test from "node:test";
import { bindFixtureSession, chooseAgentAccess, completeFirstRun, promoteFixtureOrchestrator, requireCdp } from "./setup.mjs";

test("General first run uses product completion and verifies persisted navigation unlock", async () => {
  let config = { agent: { full_access: false }, general_agent: { first_run_complete: false } };
  let completed = 0;
  const policy = { launch: async (provider, path, call) => {
    assert.equal(provider, "fixture"); assert.equal(path, "general-first-run"); return call("selected");
  } };
  const api = async (route, options) => {
    if (options) {
      assert.equal(route, "/api/agents/general/complete-first-run");
      assert.deepEqual(options, { method: "POST", body: { provider_id: "fixture", full_access: false } });
      completed++;
      config.general_agent = { first_run_complete: true, agent_provider: "fixture" };
      return { ok: true };
    }
    assert.equal(route, "/api/config");
    return config;
  };
  assert.deepEqual(await completeFirstRun(api, "fixture", policy), { provider: "fixture", first_run_complete: true });
  await completeFirstRun(api, "other", policy);
  assert.equal(completed, 1);
  await assert.rejects(completeFirstRun(async () => ({ agent: { full_access: true } }), "fixture", policy), /not persisted/);
});

test("fixture promotion explicitly targets its own tab even with a scrubbed caller environment", async () => {
  await promoteFixtureOrchestrator("owned", async (args, options) => {
    assert.deepEqual(args, ["agent", "set-orchestrator", "--tab", "owned"]);
    assert.deepEqual(options, { asTab: "owned" });
  });
});

test("fresh setup persists the real agent access choice and reads it back", async () => {
  let config = { agent: {} };
  const calls = [];
  const result = await chooseAgentAccess(async (route, options) => {
    calls.push([route, options]);
    if (options) {
      assert.deepEqual(options, { method: "PATCH", body: { key: "agent.full_access", value: true } });
      config = { agent: { full_access: true } };
    }
    return config;
  });
  assert.deepEqual(result, { full_access: true });
  assert.deepEqual(calls.map(([route]) => route), ["/api/config", "/api/config", "/api/config"]);
});

test("setup preserves either explicit access choice and refuses an unpersisted choice", async () => {
  for (const value of [false, true]) {
    assert.deepEqual(await chooseAgentAccess(async (_route, options) => {
      assert.equal(options, undefined);
      return { agent: { full_access: value } };
    }), { full_access: value });
  }
  await assert.rejects(chooseAgentAccess(async () => ({ agent: {} })), /not persisted/);
});

test("CDP prerequisite rejects absent and invalid health ports with a launch recipe", () => {
  for (const port of [undefined, null, 0, "", "invalid", -1, 65536]) {
    assert.throws(() => requireCdp({ instance: { cdp_port: port } }), /cdp_port.*CODETERM_DEV_CDP_PORT/);
  }
  assert.equal(requireCdp({ instance: { cdp_port: 9323 } }), 9323);
  assert.equal(requireCdp({ instance: { cdp_port: "9323" } }), 9323);
});

const bound = { agent_type: "fixture-provider", agent_session_id: "own-session",
  session_binding_nonce: "own-nonce", reported_session_binding_nonce: "own-nonce" };

test("fixture hook carries the explicit provider and its tab's nonce, then verifies ownership", async () => {
  let committed = false;
  await bindFixtureSession({ api: async (route, options) => {
    assert.equal(route, "/api/tabs/own-tab/hook/session_start");
    assert.deepEqual(options.body, { session_id: "own-session", provider_id: "fixture-provider",
      binding_nonce: "own-nonce", source: "core-journeys" });
    committed = true;
    return { ok: true };
  } }, "own-tab", "own-session", "fixture-provider", async () => committed ? bound : { session_binding_nonce: "own-nonce" });
});

test("fixture refuses missing ownership, mismatched readback, deferred and rejected hooks", async () => {
  const bind = (api, inspect, provider = "fixture-provider") =>
    bindFixtureSession({ api }, "own-tab", "own-session", provider, inspect);
  const ok = async () => ({ ok: true });
  await assert.rejects(bind(ok, async () => bound, ""), /provider owner/);
  await assert.rejects(bind(ok, async () => ({})), /no session binding nonce/);
  for (const patch of [{ agent_type: "foreign" }, { agent_session_id: "foreign" },
    { reported_session_binding_nonce: "foreign" }]) {
    await assert.rejects(bind(ok, async () => ({ ...bound, ...patch })), /ownership was not committed/);
  }
  await assert.rejects(bind(async () => ({ ok: true, deferred: true }), async () => bound), /not committed/);
  await assert.rejects(bind(async () => { throw new Error("HTTP 409 ownership refused"); }, async () => bound), /HTTP 409/);
});
