import assert from "node:assert/strict";
import { test } from "node:test";
import { serviceTierOptions, tierAfterModelChange } from "./serviceTierPickerModel";
import type { ProviderDescriptor } from "../../lib/types";
import {
  currentModelId,
  effortAfterModelChange,
  effortChoices,
  pickerModelGroups,
  pickerNeedsSearch,
  pickerHasPrimaryModels,
  statusLineModelName,
  planSelection,
  pickerSelection,
  statusLineSwitchGate,
} from "./statusLinePickerModel";

const live = { hasSession: true };

function provider(overrides: Partial<ProviderDescriptor> = {}): ProviderDescriptor {
  return {
    id: "vendor",
    display_name: "Vendor",
    model_switch: "next_turn",
    available_models: [
      { id: "fast", display_name: "Fast", reasoning_efforts: [] },
      {
        id: "deep",
        display_name: "Deep",
        reasoning_efforts: [{ id: "low", display_name: "Low" }, { id: "high", display_name: "High" }],
        default_reasoning_effort: "high",
      },
    ],
    reasoning_efforts: [{ id: "medium", display_name: "Medium" }],
    ...overrides,
  } as ProviderDescriptor;
}

test("gate follows the declared switch capability, never the provider id", () => {
  for (const [mode, timing] of [["live", "live"], ["live_command", "live_command"], ["next_turn", "next_turn"], ["restart_resume", "restart_resume"]] as const) {
    const gate = statusLineSwitchGate(provider({ id: "anything", model_switch: mode }), live);
    assert.equal(gate.interactive, true);
    if (gate.interactive) {
      assert.equal(gate.timing, timing);
      assert.equal(gate.confirmRestart, mode === "restart_resume");
    }
  }
  assert.equal(statusLineSwitchGate(provider({ model_switch: "unsupported" }), live).interactive, false);
  assert.equal(statusLineSwitchGate(provider({ model_switch: undefined }), live).interactive, false);
  assert.equal(statusLineSwitchGate(undefined, live).interactive, false);
});

test("role, title and protection never restrict a supported model switch", () => {
  for (const model_switch of ["live", "live_command", "next_turn", "restart_resume"] as const) {
    const descriptor = provider({ model_switch });
    for (const role of [null, "General", "Worker", "Orchestrator"]) {
      const facts = { hasSession: true, protected: true, role, title: "main" };
      assert.deepEqual(statusLineSwitchGate(descriptor, facts), statusLineSwitchGate(descriptor, live));
    }
  }
});

test("a stopped tab cannot change its model", () => {
  const gate = statusLineSwitchGate(provider(), { hasSession: false });
  assert.equal(gate.interactive, false);
  if (!gate.interactive) assert.ok(gate.reason.length > 0);
});

test("current model resolves through id or display name in priority order", () => {
  const descriptor = provider();
  assert.equal(currentModelId(descriptor, [null, "DEEP"]), "deep");
  assert.equal(currentModelId(descriptor, ["Fast", "deep"]), "fast");
  assert.equal(currentModelId(descriptor, ["vendor/unknown"]), "vendor/unknown");
  assert.equal(currentModelId(descriptor, [null, undefined]), undefined);
  assert.equal(currentModelId(descriptor, ["vendor/unknown", "deep"]), "vendor/unknown");
});

test("observed model wins after the turn ends even outside the catalogue", () => {
  const descriptor = provider();
  const launch = { agent_requested_model: "deep", agent_provider_resolved_model: "deep" };
  const observed = { ...launch, agent_session_model: "vendor/actual" };
  const resolve = (tab: Parameters<typeof statusLineModelName>[0]) =>
    currentModelId(descriptor, [statusLineModelName(tab)]);

  assert.equal(resolve(launch), "deep");
  assert.equal(resolve(observed), "vendor/actual");
  assert.equal(resolve({ ...observed }), "vendor/actual");
});

test("models group by declared section and mark the current one", () => {
  const descriptor = provider({
    available_models: [
      { id: "a1", display_name: "A one", group: "Alpha" },
      { id: "b1", display_name: "B one", group: "Beta", availability: "unavailable" },
      { id: "a2", display_name: "A two", group: "Alpha" },
    ],
  });
  const groups = pickerModelGroups(descriptor, "a2");
  assert.deepEqual(groups.map((group) => [group.label, group.rows.map((row) => row.id)]), [
    ["Alpha", ["a1", "a2"]],
    ["Beta", ["b1"]],
  ]);
  assert.deepEqual(groups.flatMap((group) => group.rows.filter((row) => row.current).map((row) => row.id)), ["a2"]);
  assert.equal(groups[1].rows[0].unavailable, true);
});

test("routed ids group under the provider's own group labels", () => {
  const descriptor = provider({
    available_models: [
      { id: "openai/gpt", display_name: "GPT" },
      { id: "anthropic/opus", display_name: "Opus" },
    ],
    model_groups: [{ id: "anthropic", label: "Anthropic" }, { id: "openai", label: "OpenAI" }],
  });
  assert.deepEqual(pickerModelGroups(descriptor, undefined).map((group) => group.label), ["OpenAI", "Anthropic"]);
});

test("flat catalogues lead with featured models and filter by query", () => {
  const descriptor = provider({
    available_models: [
      { id: "plain", display_name: "Plain" },
      { id: "star", display_name: "Star", featured: true },
    ],
  });
  assert.deepEqual(pickerModelGroups(descriptor, undefined)[0].rows.map((row) => row.id), ["star", "plain"]);
  assert.deepEqual(pickerModelGroups(descriptor, undefined, "pla")[0].rows.map((row) => row.id), ["plain"]);
  assert.deepEqual(pickerModelGroups(descriptor, undefined, "zzz"), []);
});

test("an empty catalogue still lists the model the session runs", () => {
  const empty = provider({ available_models: [] });
  assert.deepEqual(pickerModelGroups(empty, undefined, "", false, undefined), []);
  const groups = pickerModelGroups(empty, "session-model", "", false, "session-model");
  assert.deepEqual(groups.flatMap((group) => group.rows.map((row) => [row.id, row.current])), [["session-model", true]]);
  const curated = provider({ available_models: [{ id: "lead", display_name: "Lead", primary: true }] });
  assert.deepEqual(
    pickerModelGroups(curated, "lead", "", false, "session-model").flatMap((group) => group.rows.map((row) => row.id)),
    ["lead", "session-model"],
  );
  assert.deepEqual(
    pickerModelGroups(provider(), "fast", "", false, "Fast").flatMap((group) => group.rows.map((row) => row.id)),
    ["fast", "deep"],
  );
});

test("search appears only for long catalogues", () => {
  const many = Array.from({ length: 9 }, (_, index) => ({ id: `m${index}`, display_name: `M${index}` }));
  assert.equal(pickerNeedsSearch(provider()), false);
  assert.equal(pickerNeedsSearch(provider({ available_models: many })), true);
});

test("primary models curate every provider while show-all and search reach the catalogue", () => {
  const descriptor = provider({ available_models: [
    { id: "primary", display_name: "Primary 5.5", primary: true },
    { id: "older", display_name: "Older 4.5", group: "Older models" },
  ] });
  const ids = (query = "", showAll = false) => pickerModelGroups(descriptor, "older", query, showAll)
    .flatMap((group) => group.rows.map((row) => row.id));
  assert.equal(pickerHasPrimaryModels(descriptor), true);
  assert.deepEqual(ids(), ["primary"]);
  assert.deepEqual(ids("", true), ["primary", "older"]);
  assert.deepEqual(ids("4.5"), ["older"]);
  assert.deepEqual(ids("older models"), ["older"]);
  assert.deepEqual(ids("missing"), []);
  assert.equal(pickerModelGroups(descriptor, undefined)[0].rows[0].label, "Primary 5.5");
});

test("status identity always has a model, including a requested best alias", () => {
  assert.equal(statusLineModelName({ agent_requested_model: "best" }), "best");
  assert.equal(statusLineModelName({ agent_session_model: "actual", agent_provider_resolved_model: "resolved", agent_requested_model: "best" }), "actual");
  assert.equal(statusLineModelName({ agent_provider_resolved_model: "resolved" }), "resolved");
  assert.equal(statusLineModelName({}, "shell"), "shell");
  assert.equal(statusLineModelName({}), "Unknown model");
});

test("effort list follows the selected model", () => {
  const descriptor = provider();
  assert.deepEqual(effortChoices(descriptor, "fast"), []);
  assert.deepEqual(effortChoices(descriptor, "deep").map((effort) => effort.id), ["low", "high"]);
  assert.deepEqual(effortChoices(descriptor, "unknown").map((effort) => effort.id), ["medium"]);
  assert.equal(effortAfterModelChange(descriptor, "deep", "low"), "low");
  assert.equal(effortAfterModelChange(descriptor, "deep", "medium"), "high");
  assert.equal(effortAfterModelChange(descriptor, "fast", "high"), undefined);
});

test("selection applies once, confirms on restart providers, and ignores no-ops", () => {
  const nextTurn = statusLineSwitchGate(provider(), live);
  const restart = statusLineSwitchGate(provider({ model_switch: "restart_resume" }), live);
  const current = { model: "deep", effort: "high" };
  assert.deepEqual(planSelection(nextTurn, current, { model: "DEEP", effort: "high" }), { action: "none" });
  assert.deepEqual(planSelection(nextTurn, current, { model: "deep", effort: "low" }), {
    action: "apply",
    selection: { model: "deep", effort: "low" },
  });
  assert.deepEqual(planSelection(restart, current, { model: "fast" }), {
    action: "confirm",
    selection: { model: "fast" },
  });
  const plain = statusLineSwitchGate(provider({ model_switch: "unsupported" }), live);
  assert.deepEqual(planSelection(plain, current, { model: "fast" }), { action: "none" });
});

test("Fast mode is capability gated and a tier-only change applies for the next turn", () => {
  const capable = provider({ id: "arbitrary", agent_runtime: { serviceTiers: [{id:"priority",name:"Fast",description:""},{id:"ultrafast",name:"Ultrafast",description:"",unavailableReason:"Not available on your plan"}] } });
  const base = pickerSelection(capable, "deep", "high", {id:"priority"});
  assert.deepEqual(base, { model: "deep", effort: "high", serviceTier: {id:"priority"} });
  assert.deepEqual(pickerSelection(provider({ id: "codex" }), "deep", "high", {id:"priority"}), { model: "deep", effort: "high" });
  const gate = statusLineSwitchGate(capable, live);
  assert.deepEqual(planSelection(gate, base, { ...base, serviceTier: {id:null} }), {
    action: "apply", selection: { model: "deep", effort: "high", serviceTier: {id:null} },
  });
  assert.deepEqual(planSelection(gate, base, base), { action: "none" });
});

test("runtime mode is capability gated and participates in staged apply planning", () => {
  const capable = provider({ agent_runtime: {
    runtimeModes: [{ id: "auto", displayName: "Auto" }, { id: "full-access", displayName: "Full access" }],
  } });
  const current = pickerSelection(capable, "deep", "high", undefined, "auto");
  assert.deepEqual(current, { model: "deep", effort: "high", runtimeMode: "auto" });
  assert.deepEqual(planSelection(statusLineSwitchGate(capable, live), current, { ...current, runtimeMode: "full-access" }), {
    action: "apply",
    selection: { model: "deep", effort: "high", runtimeMode: "full-access" },
  });
  assert.deepEqual(pickerSelection(provider(), "deep", "high", undefined, "auto"), { model: "deep", effort: "high" });
});

test("tier choices retain provider availability and model changes clear unavailable selections", () => {
  const declared = [{id:"ultrafast",name:"Ultrafast",description:"",unavailableReason:"unknown"}];
  assert.equal(serviceTierOptions(declared,undefined)[1].disabled,true);
  assert.equal(serviceTierOptions(declared,[{id:"ultrafast",name:"Ultrafast",description:""}])[1].disabled,false);
  assert.deepEqual(tierAfterModelChange({id:"ultrafast"},undefined),{id:null});
  assert.deepEqual(tierAfterModelChange({id:"ultrafast"},[{id:"ultrafast",name:"Ultrafast",description:""}]),{id:"ultrafast"});
});
