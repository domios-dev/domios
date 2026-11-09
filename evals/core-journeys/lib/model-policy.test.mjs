import { Events } from "./env.mjs";
// WHAT: Exact advertised selections guard every paid bench boundary.
// WHY: Implicit launch defaults and restore sends can violate the owner's model policy.
// ORACLE: Injected API/command spies see explicit IDs or zero paid calls on refusal.
import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { setup, parseArgs } from "../driver.mjs";
import { dispatchWorker, review } from "../journeys/dispatch.mjs";
import { spawnOne } from "../journeys/spawn.mjs";
import { launchApiPair, uiPairChoice } from "../journeys/team.mjs";
import { completeFirstRun } from "./setup.mjs";
import { sendText } from "./tabs.mjs";
import { catalogueSelection, modelIdentity, modelSelections, preparePolicy, refreshAfterStartup } from "./model-policy.mjs";
import { fixtureProviderId } from "./fixture-control.mjs";

const provider = "arbitrary-harness", model = "vendor/exact-identity";
const descriptor = () => ({ id: provider, availability: "available", transport: "pty", catalogue_version: 12,
  model_catalogue: { authority: "authoritative", liveDiscoveryRequired: true }, available_models: [{ id: model }] });
const refreshed = () => ({ provider_id: provider, discovery_required: true, refreshed: true, version: 12, reason: null });
const record = () => ({ id: "owned", agent_type: provider, agent_requested_model: model,
  agent_provider_resolved_model: model, agent_session_model: model, agent_session_id: "own-session" });

function fixture() {
  const calls = [], descriptors = [descriptor()], profiles = [];
  const config = { general_agent: { first_run_complete: false }, agent: {}, agents: { [provider]: {} } };
  let tab = record(), refresh = refreshed();
  const ctx = {
    options: { mode: "setup", models: modelSelections([`${provider}=${model}`]) },
    state: { run: "fixture", workspace: { id: "workspace", cwd: "/owned" }, tabs: [{ id: "owned", provider, journey: "J1" }] },
    health: { build_sha: "same-build" },
    deadlines: { ready: 100 }, save() {},
    catalogueEvents: async () => ({ next: async () => { throw new Error("deadline"); }, close() {} }),
    api: async (path, options) => {
      calls.push({ path, options });
      if (path === "/api/agents/providers") return descriptors;
      if (path === "/api/agents/providers/acp/profiles") return profiles;
      if (path === "/api/agents/providers/refresh") return refresh;
      if (path === "/api/config") {
        if (options) {
          const { key, value } = options.body;
          if (key === "general_agent.model") config.general_agent.model = value;
          else if (key.split(".").length === 2) config.agents[key.split(".")[1]] = value;
          else config.agents[key.split(".")[1]].default_model = value;
        }
        return structuredClone(config);
      }
      if (path.endsWith("complete-first-run")) {
        assert.deepEqual(options.body, { provider_id: provider });
        Object.assign(config.general_agent, { first_run_complete: true, agent_provider: provider }); return { ok: true };
      }
      if (path === "/api/orchestration/new") return { opaque: "pair" };
      if (path === "/api/tabs/owned/chat-send") return { send_id: "own-send", state: "held" };
      throw new Error(`unexpected API ${path}`);
    },
    command: async (args, options) => { calls.push({ args, options }); return { tab_id: "created" }; },
  };
  return { ctx, calls, descriptors, profiles, config, setTab: (value) => { tab = value; }, setRefresh: (value) => { refresh = value; },
    prepare: async (configure = true) => { ctx.policy = await preparePolicy(ctx, { configure, required: [], readRecord: async () => tab }); return ctx.policy; } };
}

const paidCalls = (calls) => calls.filter((call) => call.args || call.path?.endsWith("/chat-send") || ["/api/agents/general/complete-first-run", "/api/orchestration/new"].includes(call.path));

function profileFixture(selection = "gemini-cli") {
  const f = fixture();
  f.descriptors.push({ id: "acp", availability: "available", agent_runtime: { launchProfiles: { "gemini-cli": {} } }, available_models: [] });
  f.profiles.push({ id: "gemini-cli", install: "detected", auth: "unknown", configOptions: [] });
  f.ctx.options.models.acp = selection;
  f.setTab({ id: "owned", agent_type: "acp", agent_runtime_config: { launchProfile: "gemini-cli" }, agent_session_id: "acp-session" });
  return f;
}

test("emitted profile spawn and dispatch flags exist in the generated CLI contract", async () => {
  const manifest = JSON.parse(readFileSync(new URL("../../../generated/cli/cli-manifest.json", import.meta.url), "utf8"));
  const f = profileFixture("gemini-cli/exact-model");
  f.profiles[0].configOptions = [{ category: "model", options: [{ value: "exact-model" }] }];
  await f.prepare();
  await spawnOne(f.ctx, { id: "acp", runtime: true });
  await dispatchWorker(f.ctx, "acp", "p", "t", "o", f.ctx.state.workspace);
  for (const { args } of paidCalls(f.calls)) {
    let command = manifest;
    let depth = 0;
    const allowed = new Set();
    for (;;) {
      for (const option of command.args) if (!option.positional) {
        allowed.add(`--${option.name}`);
        for (const alias of option.aliases) allowed.add(`--${alias}`);
      }
      const next = command.subcommands.find(child => child.name === args[depth]);
      if (!next) break;
      depth++;
      command = next;
    }
    for (const flag of args.filter(arg => arg.startsWith("--"))) assert.ok(allowed.has(flag), `${args.slice(0, depth).join(" ")} does not declare ${flag}`);
    assert.equal(args[args.indexOf("--profile") + 1], "gemini-cli");
    assert.equal(args[args.indexOf("--model") + 1], "exact-model");
  }
});

test("a failed ACP launch or send does not poison another provider's sends", async () => {
  for (const action of ["launch", "send"]) {
    const f = profileFixture(); await f.prepare();
    const failure = new Error(`ACP ${action} failed`);
    const originalCommand = f.ctx.command;
    f.ctx.command = async (args, options) => {
      if (args[2] === "acp") throw failure;
      return originalCommand(args, options);
    };
    const failed = action === "launch" ? spawnOne(f.ctx, { id: "acp", runtime: true })
      : f.ctx.policy.send("owned", () => { throw failure; });
    await assert.rejects(failed, error => error === failure);
    assert.equal(f.ctx.state.model_policy.refusal, undefined);
    f.setTab(record());
    await sendText(f.ctx, "owned", "other provider still runs");
    assert.equal(f.calls.filter(call => call.path?.endsWith("/chat-send")).length, 1);
  }
});

test("nested policy failures preserve one original root cause", async () => {
  const f = fixture(); await f.prepare();
  f.setTab({ ...record(), agent_requested_model: "wrong" });
  await assert.rejects(sendText(f.ctx, "owned", "refused"), /requested\/resolved identity/);
  const root = f.ctx.state.model_policy.refusal;
  assert.equal(root.includes("persisted refusal"), false);
  await assert.rejects(sendText(f.ctx, "owned", "still refused"), /persisted refusal/);
  assert.equal(f.ctx.state.model_policy.refusal, root);
  assert.deepEqual(paidCalls(f.calls), []);
});

test("profile-backed providers remain in the full roster and name ready selections on refusal", async () => {
  const f = profileFixture(); delete f.ctx.options.models.acp;
  await assert.rejects(f.prepare(), /complete installed provider roster; expected --model acp=<profile> \(ready profiles: gemini-cli\)/);
  assert.deepEqual(paidCalls(f.calls), []);
  assert.equal(f.calls.some(call => call.options?.method === "PATCH"), false);
});

test("profile defaults spawn and dispatch with launch_profile, no invented model or flat refresh", async () => {
  for (const selection of ["gemini-cli", "gemini-cli/default"]) {
    const f = profileFixture(selection); await f.prepare();
    assert.deepEqual(f.ctx.policy.roster.run.map(value => value.id), [provider, "acp"]);
    assert.equal(f.config.agents.acp.default_model, null);
    await spawnOne(f.ctx, { id: "acp", runtime: true });
    await dispatchWorker(f.ctx, "acp", "p", "t", "o", f.ctx.state.workspace);
    for (const { args } of paidCalls(f.calls)) {
      assert.equal(args[args.indexOf("--profile") + 1], "gemini-cli");
      assert.equal(args.includes("--model"), false);
    }
    assert.equal(f.calls.some(call => call.path === "/api/agents/providers/refresh" && call.options.body.provider_id === "acp"), false);
    const entry = f.ctx.state.model_policy.launches[0];
    assert.equal(entry.selection, selection); assert.equal(entry.launch_profile, "gemini-cli"); assert.equal(entry.model, null);
  }
});

test("explicit profile models are exact declared choices, including opaque slash IDs", async () => {
  const f = profileFixture("gemini-cli/vendor/exact");
  f.profiles[0].configOptions = [{ id: "model-id", category: "model", options: [{ value: "vendor/exact" }] }];
  await f.prepare(); await spawnOne(f.ctx, { id: "acp", runtime: true });
  const { args } = paidCalls(f.calls)[0];
  assert.equal(args[args.indexOf("--profile") + 1], "gemini-cli");
  assert.equal(args[args.indexOf("--model") + 1], "vendor/exact");
  f.setTab({ id: "owned", agent_type: "acp", agent_runtime_config: { launchProfile: "gemini-cli" },
    agent_requested_model: "vendor/exact", agent_provider_resolved_model: "vendor/exact", agent_session_model: "vendor/exact", agent_session_id: "s" });
  await f.ctx.policy.identity("owned");
  f.profiles[0].configOptions[0].options = [];
  await assert.rejects(spawnOne(f.ctx, { id: "acp" }), /unknown or ambiguous profile model/);
  assert.equal(paidCalls(f.calls).length, 1);
});

test("unknown, duplicate, uninstalled, busy and auth-blocked profiles refuse before configuration or paid work", async () => {
  for (const mutate of [
    f => { f.ctx.options.models.acp = "missing"; }, f => { f.profiles.push({ ...f.profiles[0] }); },
    f => { f.profiles[0].install = "not_installed"; }, f => { f.profiles[0].setup = { state: "installing" }; },
    f => { f.profiles[0].auth = "needs_auth"; }, f => { f.ctx.options.models.acp = "gemini-cli/guessed-model"; },
  ]) {
    const f = profileFixture(); mutate(f);
    await assert.rejects(f.prepare(), /model policy refused/);
    assert.deepEqual(paidCalls(f.calls), []);
    assert.equal(f.calls.some(call => call.options?.method === "PATCH"), false);
  }
  const f = profileFixture(); await f.prepare(); f.profiles[0].install = "not_installed";
  await assert.rejects(spawnOne(f.ctx, { id: "acp" }), /not installed and ready/);
  assert.deepEqual(paidCalls(f.calls), []);
});

test("profile default identity survives restart without claiming a model and rejects wrong profile or session", async () => {
  const f = profileFixture(); await f.prepare();
  const before = await f.ctx.policy.identity("owned");
  assert.equal(before.actual, null); assert.equal(before.requested, null); assert.equal(before.launch_profile, "gemini-cli");
  f.ctx.state = JSON.parse(JSON.stringify(f.ctx.state)); f.ctx.options.mode = "verify";
  await f.prepare(false);
  await f.ctx.policy.identity("owned", { restored: true });
  for (const patch of [{ agent_runtime_config: { launchProfile: "other" } }, { agent_session_id: "foreign" }]) {
    const g = profileFixture(); await g.prepare(); await g.ctx.policy.identity("owned");
    g.setTab({ id: "owned", agent_type: "acp", agent_runtime_config: { launchProfile: "gemini-cli" }, agent_session_id: "acp-session", ...patch });
    await assert.rejects(g.ctx.policy.identity("owned", { restored: true }), /profile.*(mismatched|changed)/);
  }
});

test("the run's control fixture never narrows the complete real provider roster", async () => {
  const f = fixture();
  const control = fixtureProviderId(f.ctx.state.run);
  f.ctx.state.j3 = { fixture_provider: control };
  f.descriptors.push({ id: control, availability: "available" });
  await f.prepare();
  assert.deepEqual(f.ctx.policy.roster.run.map(value => value.id), [provider]);
  f.descriptors.push({ ...descriptor(), id: "another-real-provider" });
  let launches = 0;
  await assert.rejects(f.ctx.policy.launch(provider, "must-refuse", () => { launches++; }), /roster changed/);
  assert.equal(launches, 0);
  const spoofed = fixture();
  spoofed.ctx.state.j3 = { fixture_provider: provider };
  await assert.rejects(spoofed.prepare(), /control fixture identity differs/);
  assert.deepEqual(paidCalls(spoofed.calls), []);
});

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

test("flat launch validation observes models after the refresh completes", async () => {
  const f = fixture(); await f.prepare(); f.calls.length = 0;
  const original = f.ctx.api;
  f.ctx.api = async (path, options) => {
    if (path === "/api/agents/providers/refresh") f.descriptors[0] = { ...descriptor(), available_models: [] };
    return original(path, options);
  };
  await assert.rejects(spawnOne(f.ctx, { id: provider }), /catalogue absent/);
  assert.deepEqual(paidCalls(f.calls), []);
});

test("only initial J1 input may precede actual model evidence; unknown then blocks every later send", async () => {
  const f = fixture(); await f.prepare(); f.calls.length = 0;
  const unknown = record(); delete unknown.agent_session_model; f.setTab(unknown);
  await sendText(f.ctx, "owned", "first");
  await assert.rejects(sendText(f.ctx, "owned", "second"), /actual model unknown/);
  await assert.rejects(spawnOne(f.ctx, { id: provider }), /persisted refusal/);
  assert.equal(paidCalls(f.calls).length, 1);
  assert.equal(paidCalls(f.calls)[0].path, "/api/tabs/owned/chat-send");
  assert.equal(paidCalls(f.calls)[0].options.body.text, "first");
  assert.ok(paidCalls(f.calls)[0].options.body.client_token);
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

async function restoredFixture(identity = record()) {
  const f = fixture(); await f.prepare(); f.setTab(identity); await f.ctx.policy.identity("owned");
  f.ctx.state = JSON.parse(JSON.stringify(f.ctx.state)); f.ctx.options.mode = "verify";
  await f.prepare(false); f.calls.length = 0;
  return f;
}
const withoutActual = (value = record()) => { const unknown = { ...value }; delete unknown.agent_session_model; return unknown; };
// Each event models one daemon state change; an exhausted list is the wait's deadline.
function modelEventsFrom(steps) {
  const opened = { count: 0, closed: 0 };
  const open = async () => {
    opened.count++;
    return { next: async () => { const step = steps.shift(); if (!step) throw new Error("deadline"); step(); return { event_type: "mux-state-changed" }; },
      drain() {}, close() { opened.closed++; } };
  };
  return { open, opened };
}

test("a restored tab's actual model arriving after readiness is confirmed, with no persisted refusal", async () => {
  const f = await restoredFixture();
  f.setTab(withoutActual());
  const { open, opened } = modelEventsFrom([() => {}, () => f.setTab(record())]);
  f.ctx.modelEvents = open;
  const evidence = await f.ctx.policy.identity("owned", { restored: true });
  assert.equal(evidence.actual, model);
  assert.equal(f.ctx.state.model_policy.refusal, undefined);
  assert.equal(f.ctx.state.model_policy.restored_identities.owned.actual, model);
  assert.deepEqual(opened, { count: 1, closed: 1 });
  await sendText(f.ctx, "owned", "restored turn runs on the confirmed model");
  assert.equal(paidCalls(f.calls).length, 1);
});

test("a restored send holds its paid call until the actual model is confirmed", async () => {
  const f = await restoredFixture();
  f.setTab(withoutActual());
  let paidBeforeConfirmation = null;
  const { open } = modelEventsFrom([() => { paidBeforeConfirmation = paidCalls(f.calls).length; f.setTab(record()); }]);
  f.ctx.modelEvents = open;
  await sendText(f.ctx, "owned", "must wait for confirmation");
  assert.equal(paidBeforeConfirmation, 0);
  assert.equal(paidCalls(f.calls).length, 1);
});

test("an actual model still unknown at the deadline refuses, persists and pays nothing", async () => {
  const f = await restoredFixture();
  f.setTab(withoutActual());
  const { open, opened } = modelEventsFrom([]);
  f.ctx.modelEvents = open;
  await assert.rejects(f.ctx.policy.identity("owned", { restored: true }), /actual model unconfirmed/);
  assert.match(f.ctx.state.model_policy.refusal, /actual model unconfirmed/);
  assert.deepEqual(opened, { count: 1, closed: 1 });
  await assert.rejects(sendText(f.ctx, "owned", "must not pay"), /persisted refusal/);
  assert.deepEqual(paidCalls(f.calls), []);
});

test("a mismatch reported during the wait refuses at once", async () => {
  for (const patch of [{ agent_session_model: "other" }, { model_fallback: {} }, { model_identity_mismatch: {} },
    { agent_requested_model: "other" }, { agent_session_id: "foreign-session" }]) {
    const f = await restoredFixture();
    f.setTab(withoutActual());
    const later = [() => assert.fail("a settled identity must not wait again")];
    const { open } = modelEventsFrom([() => f.setTab({ ...withoutActual(), ...patch }), ...later]);
    f.ctx.modelEvents = open;
    await assert.rejects(f.ctx.policy.identity("owned", { restored: true }), /model policy refused/);
    assert.ok(f.ctx.state.model_policy.refusal);
    assert.doesNotMatch(f.ctx.state.model_policy.refusal, /unconfirmed/);
    assert.deepEqual(paidCalls(f.calls), []);
  }
});

test("only an absent actual model waits: any other disagreement refuses without subscribing", async () => {
  for (const patch of [{ agent_session_model: "other" }, { agent_requested_model: "other" }, { agent_provider_resolved_model: "other" },
    { model_fallback: {} }, { model_identity_mismatch: {} }, { agent_session_id: "foreign-session" }]) {
    const f = await restoredFixture();
    f.setTab({ ...withoutActual(), ...patch });
    const { open, opened } = modelEventsFrom([]);
    f.ctx.modelEvents = open;
    await assert.rejects(f.ctx.policy.identity("owned", { restored: true }), /model policy refused/);
    assert.equal(opened.count, 0);
  }
});

test("a confirmed restored tab and an unrestored tab never wait", async () => {
  const f = await restoredFixture();
  const { open, opened } = modelEventsFrom([]);
  f.ctx.modelEvents = open;
  assert.equal((await f.ctx.policy.identity("owned", { restored: true })).actual, model);
  f.setTab(withoutActual());
  await assert.rejects(f.ctx.policy.identity("owned"), /actual model unknown/);
  assert.equal(opened.count, 0);
});

test("restored profile sessions wait for a model they selected or already observed, and a default without one never waits", async () => {
  const profileTab = { id: "owned", agent_type: "acp", agent_runtime_config: { launchProfile: "gemini-cli" }, agent_session_id: "acp-session" };
  const restoredProfile = async (selection, before, after) => {
    const f = profileFixture(selection);
    f.profiles[0].configOptions = [{ category: "model", options: [{ value: "exact" }] }];
    await f.prepare(); f.setTab(before); await f.ctx.policy.identity("owned");
    f.ctx.state = JSON.parse(JSON.stringify(f.ctx.state)); f.ctx.options.mode = "verify";
    await f.prepare(false);
    f.setTab(after);
    return f;
  };
  const explicit = { ...profileTab, agent_requested_model: "exact", agent_provider_resolved_model: "exact", agent_session_model: "exact" };
  const observed = { ...profileTab, agent_provider_resolved_model: "auto", agent_session_model: "auto" };
  for (const [selection, before] of [["gemini-cli/exact", explicit], ["gemini-cli", observed]]) {
    const f = await restoredProfile(selection, before, withoutActual(before));
    const { open, opened } = modelEventsFrom([() => f.setTab(before)]);
    f.ctx.modelEvents = open;
    assert.equal((await f.ctx.policy.identity("owned", { restored: true })).actual, before.agent_session_model);
    assert.equal(opened.count, 1);
    assert.equal(f.ctx.state.model_policy.refusal, undefined);
  }
  const f = await restoredProfile("gemini-cli", profileTab, profileTab);
  const { open, opened } = modelEventsFrom([]);
  f.ctx.modelEvents = open;
  assert.equal((await f.ctx.policy.identity("owned", { restored: true })).actual, null);
  assert.equal(opened.count, 0);
});

test("identity checks exact requested, catalogue-resolved and runtime identities independently", () => {
  assert.equal(modelIdentity(record(), provider, model).actual, model);
  for (const field of ["agent_requested_model", "agent_provider_resolved_model", "agent_session_model"]) {
    const value = record(); delete value[field];
    assert.throws(() => modelIdentity(value, provider, model), /unknown/);
  }
});


test("startup catalogue waits only on a newer event for its own provider before one final refresh", async () => {
  const calls = [], changes = [
    { event_type: "mux-state-changed" },
    { event_type: "model-catalogue-changed", data: { provider: "other", version: 99 } },
    { event_type: "model-catalogue-changed", data: { provider, version: 12 } },
    { event_type: "model-catalogue-changed", data: { provider, version: 13 } },
  ];
  let closed = false;
  const ctx = {
    catalogueEvents: async () => {
      calls.push("subscribe");
      return { next: async () => { calls.push("event"); return changes.shift(); }, close: () => { closed = true; } };
    },
    api: async (_path, options) => {
      assert.deepEqual(options.body, { provider_id: provider });
      calls.push("refresh");
      return calls.filter(value => value === "refresh").length === 1
        ? { ...refreshed(), refreshed: false } : { ...refreshed(), version: 13 };
    },
  };
  assert.equal((await refreshAfterStartup(ctx, provider)).refreshed, true);
  assert.deepEqual(calls, ["subscribe", "refresh", "event", "event", "event", "event", "refresh"]);
  assert.equal(closed, true);
});

test("completed refresh needs no future change event and closes its observation", async () => {
  let closed = false, calls = 0;
  const ctx = {
    catalogueEvents: async () => ({ next: async () => { assert.fail("completed refresh must not wait"); }, close: () => { closed = true; } }),
    api: async () => { calls++; return refreshed(); },
  };
  assert.deepEqual(await refreshAfterStartup(ctx, provider), refreshed());
  assert.equal(calls, 1);
  assert.equal(closed, true);
});

test("missing startup catalogue event fails explicitly without another refresh or paid launch", async () => {
  const f = fixture();
  f.setRefresh({ ...refreshed(), refreshed: false });
  await assert.rejects(f.prepare(), /startup catalogue completion event did not arrive before the 15 s deadline/);
  assert.equal(f.calls.filter(call => call.path === "/api/agents/providers/refresh").length, 1);
  assert.deepEqual(paidCalls(f.calls), []);
});


test("startup completion followed by a refused refresh never loops or weakens model policy", async () => {
  const f = fixture();
  f.setRefresh({ ...refreshed(), refreshed: false });
  f.ctx.catalogueEvents = async () => ({
    next: async () => ({ event_type: "model-catalogue-changed", data: { provider, version: 13 } }), close() {},
  });
  await assert.rejects(f.prepare(), /catalogue refresh unavailable/);
  assert.equal(f.calls.filter(call => call.path === "/api/agents/providers/refresh").length, 2);
  assert.deepEqual(paidCalls(f.calls), []);
});

test("startup catalogue subscription deadline reports the provider before any refresh", async () => {
  const f = fixture();
  f.ctx.catalogueEvents = async () => { throw new Error("deadline"); };
  await assert.rejects(f.prepare(), new RegExp(`${provider} catalogue refresh response did not arrive before the 45 s deadline`));
  assert.equal(f.calls.some(call => call.path === "/api/agents/providers/refresh"), false);
});


test("verify can qualify an unchanged cached model using this run's live setup on the same build", async () => {
  const f = fixture(); await f.prepare();
  f.ctx.state = JSON.parse(JSON.stringify(f.ctx.state));
  f.ctx.options.mode = "verify";
  f.setRefresh({ ...refreshed(), refreshed: false, reason: "the provider reported no live model list" });
  await f.prepare(false);
  const entry = f.ctx.state.model_policy.verify_catalogues[provider];
  assert.equal(entry.refresh.refreshed, false);
  assert.equal(entry.model.id, model);
  assert.deepEqual(entry.qualification, { verified_live_in_setup: true, post_restart_refresh_in_flight: true,
    model_present_in_cache: true, model_id: model, run: "fixture", build_sha: "same-build" });
});

test("verify cache qualification refuses missing live setup, a different build/run/model and an absent cached model", async () => {
  for (const mutate of [
    f => { delete f.ctx.state.model_policy.setup_live[provider]; },
    f => { f.ctx.state.model_policy.setup_live[provider].catalogue.refresh.refreshed = false; },
    f => { f.ctx.health.build_sha = "different-build"; },
    f => { f.ctx.state.model_policy.setup_live[provider].run = "other-run"; },
    f => { f.ctx.state.model_policy.setup_live[provider].catalogue.selected = "other-model"; },
    f => { f.descriptors[0].available_models = []; },
    f => { f.setRefresh({ ...refreshed(), refreshed: false, reason: "discovery timed out after 30s" }); },
  ]) {
    const f = fixture(); await f.prepare(); f.ctx.options.mode = "verify";
    f.setRefresh({ ...refreshed(), refreshed: false }); mutate(f); f.calls.length = 0;
    await assert.rejects(f.prepare(false), /model policy refused/);
    assert.deepEqual(paidCalls(f.calls), []);
    assert.equal(f.calls.filter(call => call.path === "/api/agents/providers/refresh").length, 1);
  }
});


test("a cached refresh response arriving after 15 s starts its own later event window and qualifies verify", async () => {
  const f = fixture(); await f.prepare(); f.ctx.options.mode = "verify";
  const bounds = [], eventDeadline = new AbortController();
  let elapsed = 0;
  f.ctx.catalogueDeadline = ms => {
    bounds.push({ ms, at: elapsed });
    return ms === 15_000 ? eventDeadline.signal : new AbortController().signal;
  };
  const original = f.ctx.api;
  f.ctx.api = async (path, options) => {
    if (path === "/api/agents/providers/refresh") {
      assert.deepEqual(bounds, [{ ms: 45_000, at: 0 }]);
      elapsed = 20_000;
      assert.equal(options.signal.aborted, false);
      return { ...refreshed(), refreshed: false };
    }
    return original(path, options);
  };
  f.ctx.catalogueEvents = async () => ({
    next: async signal => {
      assert.equal(signal, eventDeadline.signal);
      assert.deepEqual(bounds, [{ ms: 45_000, at: 0 }, { ms: 15_000, at: 20_000 }]);
      eventDeadline.abort(); throw new Error("deadline");
    }, close() {},
  });
  await f.prepare(false);
  assert.equal(f.ctx.state.model_policy.verify_catalogues[provider].qualification.verified_live_in_setup, true);
});

test("a missing refresh response refuses at its own bound and never qualifies a cached verify", async () => {
  const f = fixture(); await f.prepare(); f.ctx.options.mode = "verify";
  const request = new AbortController(), bounds = [];
  f.ctx.catalogueDeadline = ms => { bounds.push(ms); return request.signal; };
  const original = f.ctx.api;
  f.ctx.api = async (path, options) => {
    if (path === "/api/agents/providers/refresh") { request.abort(); throw new Error("deadline"); }
    return original(path, options);
  };
  await assert.rejects(f.prepare(false), /catalogue refresh response did not arrive before the 45 s deadline/);
  assert.deepEqual(bounds, [45_000]);
  assert.equal(f.ctx.state.model_policy.verify_catalogues, undefined);
});

test("an event subscription can use a separate per-wait abort signal without timing out its lifetime", async () => {
  const lifetime = new AbortController(), event = new AbortController();
  const events = new Events({}, lifetime.signal);
  const pending = events.next(event.signal);
  event.abort();
  await assert.rejects(pending, /deadline/);
  assert.equal(lifetime.signal.aborted, false);
  assert.equal(events.waiter, null);
});
