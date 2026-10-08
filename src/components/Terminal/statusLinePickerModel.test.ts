import assert from "node:assert/strict";
import { test } from "node:test";
import type { ProviderDescriptor } from "../../lib/types";
import {
  currentModelId,
  effortAfterModelChange,
  effortChoices,
  pickerAnchor,
  pickerModelGroups,
  pickerNeedsSearch,
  planSelection,
  statusLineSwitchGate,
} from "./statusLinePickerModel";

const live = { hasSession: true, protected: false };

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

test("gate explains why a protected or stopped tab stays plain text", () => {
  const protectedGate = statusLineSwitchGate(provider(), { hasSession: true, protected: true });
  const stoppedGate = statusLineSwitchGate(provider(), { hasSession: false, protected: false });
  assert.equal(protectedGate.interactive, false);
  assert.equal(stoppedGate.interactive, false);
  if (!protectedGate.interactive && !stoppedGate.interactive) {
    assert.ok(protectedGate.reason.length > 0);
    assert.notEqual(protectedGate.reason, stoppedGate.reason);
  }
});

test("current model resolves through id or display name in priority order", () => {
  const descriptor = provider();
  assert.equal(currentModelId(descriptor, [null, "DEEP"]), "deep");
  assert.equal(currentModelId(descriptor, ["Fast", "deep"]), "fast");
  assert.equal(currentModelId(descriptor, ["vendor/unknown"]), "vendor/unknown");
  assert.equal(currentModelId(descriptor, [null, undefined]), undefined);
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

test("search appears only for long catalogues", () => {
  const many = Array.from({ length: 9 }, (_, index) => ({ id: `m${index}`, display_name: `M${index}` }));
  assert.equal(pickerNeedsSearch(provider()), false);
  assert.equal(pickerNeedsSearch(provider({ available_models: many })), true);
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

test("anchor opens upward and stays inside the viewport", () => {
  assert.deepEqual(pickerAnchor({ left: 40, top: 500 }, { width: 1000, height: 800 }, 300), { left: 40, bottom: 306 });
  assert.deepEqual(pickerAnchor({ left: 900, top: 500 }, { width: 1000, height: 800 }, 300), { left: 692, bottom: 306 });
  assert.deepEqual(pickerAnchor({ left: -20, top: 900 }, { width: 320, height: 800 }, 300), { left: 8, bottom: 8 });
});
