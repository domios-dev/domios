import { describe, expect, it } from "vitest";
import type { ProviderDescriptor } from "./types";
import type { RuntimeSettings } from "@codeterm/plugin-sdk";
import { pickerSections } from "./pickerSettings";
import modelsOnlyFixture from "../../plugins/provider-acp/tests/fixtures/runtime/session-models-only.json";

const native = { available_models: [{ id: "native-model", display_name: "Native model", reasoning_efforts: [{ id: "high", display_name: "High" }] }], agent_runtime: {
  runtimeModes: [{ id: "plan", displayName: "Plan" }], serviceTiers: [{ id: "fast", name: "Fast" }],
} } as unknown as ProviderDescriptor;
const option = (id: string, category?: string) => ({ id, name: id, category, currentValue: "one", options: [{ value: "one", name: "One" }, { value: "two", name: "Two" }] });

describe("provider settings sections", () => {
  it("orders native model, effort, runtime mode and speed with their apply targets", () => {
    const sections = pickerSections({ descriptor: native, model: "native-model", effort: "high", runtimeMode: "plan", serviceTier: { id: "fast" } });
    expect(sections.map(section => section.category)).toEqual(["model", "effort", "mode", "speed"]);
    expect(sections.map(section => section.applyTarget.kind)).toEqual(["model", "effort", "mode", "speed"]);
    expect(sections.map(section => section.current)).toEqual(["native-model", "high", "plan", '"fast"']);
    expect(sections[3].options.map(value => value.label)).toEqual(["Default", "Fast"]);
  });

  it("deduplicates declared mode against legacy and native modes and keeps generic extras in order", () => {
    const controls: RuntimeSettings = { configOptions: [option("Model", "model"), option("Fast mode"), option("Mode", "mode"), option("Ultracode"), option("Agent"), option("Other mode", "mode")], modes: [{ id: "legacy", name: "Legacy" }], mode: "legacy" };
    const sections = pickerSections({ descriptor: native, controls });
    expect(sections.filter(section => section.category === "mode")).toEqual([expect.objectContaining({ id: "Mode", applyTarget: { kind: "config", id: "Mode" } })]);
    expect(sections.filter(section => !section.category).map(section => section.id)).toEqual(["Fast mode", "Ultracode", "Agent"]);
    expect(sections.filter(section => section.category === "model")).toHaveLength(1);
    expect(pickerSections({ controls: { ...controls, configOptions: [] } })[0]).toMatchObject({ category: "mode", current: "legacy", applyTarget: { kind: "session" } });
    expect(pickerSections({ controls: { ...controls, configOptions: [{ ...option("Mode", "mode"), options: [] }] } })).toHaveLength(1);
  });

  it("leaves a declared model and effort alone with no extra settings", () => {
    const sections = pickerSections({ controls: { configOptions: [option("opaque-one", "model"), option("opaque-two", "thought_level")] } });
    expect(sections.map(section => section.category)).toEqual(["model", "effort"]);
    expect(sections.map(section => section.applyTarget)).toEqual([{ kind: "config", id: "opaque-one" }, { kind: "config", id: "opaque-two" }]);
  });

  it("shows the models-only ACP fixture as one model section without effort", () => {
    const loaded = modelsOnlyFixture.expected.results[1].step.result;
    const controls = ("settings" in loaded ? loaded.settings : {}) as RuntimeSettings;
    const sections = pickerSections({ controls });
    expect(sections).toHaveLength(1);
    expect(sections[0]).toMatchObject({ category: "model", current: "wire-one", applyTarget: { kind: "config", id: "acp-session-model" } });
    expect(sections[0].options.map(option => [option.value, option.label])).toEqual([["wire-one", "First declared"], ["wire-two", "Second declared"]]);
  });

  it("does not infer categories or boolean types from ids, names or values", () => {
    const sections = pickerSections({ controls: { configOptions: [{ ...option("model"), options: [{value:"on", name:"On"}, {value:"off", name:"Off"}] }] } });
    expect(sections[0]).toMatchObject({ category: undefined, kind: "select" });
    expect(pickerSections({ descriptor: { available_models: [] } as unknown as ProviderDescriptor })).toEqual([]);
  });
});
