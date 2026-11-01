// WHAT: Exact advertised selections guard every paid bench boundary.
// WHY: Implicit launch defaults and restore sends can violate the owner's model policy.
// ORACLE: Injected API/command spies see explicit IDs or zero paid calls on refusal.
import assert from "node:assert/strict";
import test from "node:test";
import { setup, parseArgs } from "../driver.mjs";
import { dispatchWorker, review } from "../journeys/dispatch.mjs";
import { spawnOne } from "../journeys/spawn.mjs";
import { launchApiPair, uiPairChoice } from "../journeys/team.mjs";
import { completeFirstRun } from "./setup.mjs";
import { sendText } from "./tabs.mjs";
import { catalogueSelection, modelIdentity, modelSelections, preparePolicy } from "./model-policy.mjs";

const provider = "arbitrary-harness", model = "vendor/exact-identity";
const descriptor = () => ({ id: provider, availability: "available", transport: "pty", catalogue_version: 12,
  model_catalogue: { authority: "authoritative", liveDiscoveryRequired: true }, available_models: [{ id: model }] });
const refreshed = () => ({ provider_id: provider, discovery_required: true, refreshed: true, version: 12, reason: null });
const record = () => ({ id: "owned", agent_type: provider, agent_requested_model: model,
  agent_provider_resolved_model: model, agent_session_model: model, agent_session_id: "own-session" });

function fixture() {
  const calls = [], descriptors = [descriptor()];
  const config = { general_agent: { first_run_complete: false }, agent: { full_access: false }, agents: { [provider]: {} } };
  let tab = record(), refresh = refreshed();
  const ctx = {
    options: { mode: "setup", models: modelSelections([`${provider}=${model}`]) },
    state: { run: "fixture", workspace: { id: "workspace", cwd: "/owned" }, tabs: [{ id: "owned", provider, journey: "J1" }] },
    deadlines: { ready: 100 }, save() {},
    api: async (path, options) => {
      calls.push({ path, options });
      if (path === "/api/agents/providers") return descriptors;
      if (path === "/api/agents/providers/refresh") return refresh;
      if (path === "/api/config") {
        if (options) {
          const { key, value } = options.body;
          if (key === "general_agent.model") config.general_agent.model = value;
          else config.agents[key.split(".")[1]].default_model = value;
        }
        return structuredClone(config);
      }
      if (path.endsWith("complete-first-run")) { Object.assign(config.general_agent, { first_run_complete: true, agent_provider: provider }); return { ok: true }; }
      if (path === "/api/orchestration/new") return { opaque: "pair" };
      throw new Error(`unexpected API ${path}`);
    },
    command: async (args, options) => { calls.push({ args, options }); return { tab_id: "created" }; },
    sendCommand: async (args, options) => { calls.push({ args, options }); return { code: 0, stdout: "sent" }; },
  };
  return { ctx, calls, descriptors, config, setTab: (value) => { tab = value; }, setRefresh: (value) => { refresh = value; },
    prepare: async (configure = true) => { ctx.policy = await preparePolicy(ctx, { configure, required: [], readRecord: async () => tab }); return ctx.policy; } };
}

const paidCalls = (calls) => calls.filter((call) => call.args || ["/api/agents/general/complete-first-run", "/api/orchestration/new"].includes(call.path));

test("repeat selectors are exact and duplicate/conflicting/malformed selections are refused", () => {
  assert.equal(parseArgs(["setup", "--out", "owned", "--model", `${provider}=${model}`]).models[provider], model);
  for (const entries of [["x=a", "x=a"], ["x=a", "x=b"], ["x"], ["=a"], ["x="], ["x= a"], [undefined], ["x=a=b"]]) {
    assert.throws(() => modelSelections(entries), /model policy refused/);
  }
  assert.equal(modelSelections(["__proto__=exact"]).__proto__, "exact");
});

test("catalogue rejects absent, unknown, duplicate and unavailable models without aliases or defaults", () => {
  for (const mutate of [
    (d) => { delete d.model_catalogue; }, (d) => { delete d.available_models; },
    (d) => { d.available_models = []; }, (d) => { d.available_models = [{ id: "default" }]; },
    (d) => { d.available_models.push({ id: model }); }, (d) => { d.available_models[0].available = false; },
    (d) => { d.available_models[0].availability = "unknown"; }, (d) => { d.availability = "unavailable"; },
  ]) {
    const d = descriptor(); mutate(d);
    assert.throws(() => catalogueSelection(d, model, refreshed()), /model policy refused/);
  }
  assert.throws(() => catalogueSelection(descriptor(), "alias", refreshed()), /unknown/);
  assert.throws(() => catalogueSelection(descriptor(), model, { ...refreshed(), refreshed: false }), /refresh unavailable/);
  assert.throws(() => catalogueSelection(descriptor(), model, { ...refreshed(), reason: "timeout" }), /refresh unavailable/);
  const staticDescriptor = descriptor(); staticDescriptor.model_catalogue.liveDiscoveryRequired = false;
  assert.equal(catalogueSelection(staticDescriptor, model, { ...refreshed(), discovery_required: false,
    refreshed: false, reason: "the provider reported no live model list" }).selected, model);
});

test("policy rejection calls neither first-run nor spawn nor send", async () => {
  for (const mutate of [
    (f) => { f.ctx.options.models = {}; },
    (f) => { f.ctx.options.models[provider] = "unknown"; },
    (f) => { f.descriptors[0].available_models = []; },
    (f) => { f.setRefresh({ ...refreshed(), refreshed: false }); },
    (f) => { f.config.general_agent.model = "expensive-override"; },
    (f) => { f.ctx.options.providers = ["narrowed"]; },
  ]) {
    const f = fixture(); mutate(f);
    await assert.rejects(async () => {
      await f.prepare();
      await completeFirstRun(f.ctx.api, provider, f.ctx.policy);
      await spawnOne(f.ctx, { id: provider });
      await sendText(f.ctx, "owned", "must not pay");
    }, /model policy refused/);
    assert.deepEqual(paidCalls(f.calls), []);
  }
  const f = fixture();
  f.ctx.options.models = {};
  await assert.rejects(setup(f.ctx), /model policy refused/);
  assert.deepEqual(paidCalls(f.calls), []);
  assert.equal(f.calls.some((call) => call.options?.method === "PATCH"), false);
});

test("General, J1, both J2 roles and J3 receive the exact explicit model", async () => {
  const f = fixture(); await f.prepare();
  await completeFirstRun(f.ctx.api, provider, f.ctx.policy);
  await spawnOne(f.ctx, { id: provider, runtime: false });
  await launchApiPair(f.ctx, provider);
  const choice = await uiPairChoice(f.ctx.policy, provider);
  assert.deepEqual(choice, { mode: "pair", orchestratorProviderId: provider, plannerProviderId: provider,
    orchestratorModelId: model, plannerModelId: model });
  await dispatchWorker(f.ctx, provider, "plan", "task", "orchestrator", f.ctx.state.workspace);
  assert.equal(f.config.general_agent.model, model);
  assert.equal(f.config.agents[provider].default_model, model);
  const paid = paidCalls(f.calls);
  assert.equal(paid.length, 4);
  for (const call of paid.filter((call) => call.args)) assert.equal(call.args[call.args.indexOf("--model") + 1], model);
  const pair = paid.find((call) => call.path === "/api/orchestration/new").options.body;
  assert.equal(pair.orchestrator_model, model);
  assert.equal(pair.planner_model, model);
  const first = f.calls.findIndex((call) => call.path?.endsWith("complete-first-run"));
  assert.ok(f.calls.findIndex((call) => call.options?.body?.key === "general_agent.model") < first);
  assert.ok(f.calls.slice(0, first).some((call) => call.path === "/api/agents/providers/refresh"));
});

test("fresh rejection at each launch boundary causes zero paid calls", async () => {
  for (const launch of [
    (ctx) => completeFirstRun(ctx.api, provider, ctx.policy),
    (ctx) => spawnOne(ctx, { id: provider }),
    (ctx) => launchApiPair(ctx, provider),
    (ctx) => uiPairChoice(ctx.policy, provider),
    (ctx) => dispatchWorker(ctx, provider, "p", "t", "o", ctx.state.workspace),
  ]) {
    const f = fixture(); await f.prepare(); f.calls.length = 0;
    f.descriptors[0].available_models[0].availability = "unknown";
    await assert.rejects(launch(f.ctx), /capability unavailable or unknown/);
    assert.deepEqual(paidCalls(f.calls), []);
  }
});

test("only initial J1 input may precede actual model evidence; unknown then blocks every later send", async () => {
  const f = fixture(); await f.prepare(); f.calls.length = 0;
  const unknown = record(); delete unknown.agent_session_model; f.setTab(unknown);
  await sendText(f.ctx, "owned", "first");
  await assert.rejects(sendText(f.ctx, "owned", "second"), /actual model unknown/);
  await assert.rejects(spawnOne(f.ctx, { id: provider }), /persisted refusal/);
  assert.equal(paidCalls(f.calls).length, 1);
  assert.equal(paidCalls(f.calls)[0].options.stdin, "first");
});

test("mismatch, fallback, missing identity and conflicting General block send and review bounce", async () => {
  for (const patch of [{ agent_requested_model: null }, { agent_provider_resolved_model: "other" },
    { agent_session_model: "other" }, { model_fallback: {} }, { model_identity_mismatch: {} }]) {
    const f = fixture(); await f.prepare(); f.calls.length = 0;
    f.setTab({ ...record(), ...patch });
    await assert.rejects(sendText(f.ctx, "owned", "must not pay"), /model policy refused/);
    f.ctx.state.j3 = { plan: "p", task: "t", orchestrator: "o", worker: "owned" };
    await assert.rejects(review(f.ctx, "bounce", "must not pay"), /persisted refusal/);
    assert.deepEqual(paidCalls(f.calls), []);
  }
  const f = fixture(); await f.prepare(); f.config.general_agent.model = "other";
  await assert.rejects(sendText(f.ctx, "owned", "must not pay"), /conflicting General/);
  assert.deepEqual(paidCalls(f.calls), []);
});

test("persisted policy survives JSON restart and refuses changed selection, roster, session or absent identity", async () => {
  const f = fixture(); await f.prepare(); await f.ctx.policy.identity("owned");
  f.ctx.state = JSON.parse(JSON.stringify(f.ctx.state)); f.ctx.options.mode = "verify";
  await f.prepare(false); f.calls.length = 0;
  await sendText(f.ctx, "owned", "restored");
  assert.equal(paidCalls(f.calls).length, 1);
  assert.equal(f.ctx.state.model_policy.identities.owned.session, "own-session");
  f.setTab({ ...record(), agent_session_id: "foreign-session" }); f.calls.length = 0;
  await assert.rejects(sendText(f.ctx, "owned", "must not pay"), /policy session changed/);
  assert.deepEqual(paidCalls(f.calls), []);
  for (const mutate of [
    (g) => { g.ctx.options.models[provider] = "other"; },
    (g) => { g.ctx.state.model_policy.roster = []; },
    (g) => { delete g.ctx.state.model_policy; },
  ]) {
    const g = fixture(); await g.prepare(); mutate(g);
    await assert.rejects(g.prepare(false), /persisted restart policy/);
    assert.deepEqual(paidCalls(g.calls), []);
  }
});

test("identity checks exact requested, catalogue-resolved and runtime identities independently", () => {
  assert.equal(modelIdentity(record(), provider, model).actual, model);
  for (const field of ["agent_requested_model", "agent_provider_resolved_model", "agent_session_model"]) {
    const value = record(); delete value[field];
    assert.throws(() => modelIdentity(value, provider, model), /unknown/);
  }
});
