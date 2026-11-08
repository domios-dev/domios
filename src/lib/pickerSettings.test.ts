import { describe, expect, it } from "vitest";
import type { ProviderDescriptor } from "./types";
import type { RuntimeSettings } from "@codeterm/plugin-sdk";
import { hostApprovalsChoice, hostFullAccessOverride, pickerSections } from "./pickerSettings";
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

  const others: RuntimeSettings = { mode: "default", modes: [{ id: "default", name: "Default" }] };

  it("adds the Permissions row beside the mode row with both choices", () => {
    const sections = pickerSections({ controls: others, hostApprovals: "ask" });
    expect(sections.map(section => section.label)).toEqual(["Mode", "Permissions"]);
    const row = sections[1];
    expect(row.current).toBe("ask");
    expect(row.options.map(option => [option.value, option.label])).toEqual([["ask", "Ask each time"], ["allow_all", "Allow all"]]);
    expect(row.applyTarget).toEqual({ kind: "host_approvals" });
  });

  it("adds nothing without a host choice", () => {
    expect(pickerSections({ controls: others }).map(section => section.label)).toEqual(["Mode"]);
  });
});

describe("hostApprovalsChoice", () => {
  const descriptor = (hostApprovals: boolean) => ({
    id: "opaque", display_name: "Opaque", available_models: [],
    agent_runtime: { hostApprovals, launchProfiles: {
      plain: { binary: "opaque", args: [] },
      native: { binary: "opaque", args: [], fullAccessMode: "approve-all" },
    } },
  }) as unknown as ProviderDescriptor;
  const advertised: RuntimeSettings = { mode: "default", modes: [{ id: "default", name: "Default" }, { id: "approve-all", name: "Approve all" }] };
  const others: RuntimeSettings = { mode: "default", modes: [{ id: "default", name: "Default" }] };

  it.each([
    ["no saved choice defaults to no questions", true, { launchProfile: "plain" }, undefined, "allow_all"],
    ["a saved ask is kept", true, { launchProfile: "plain", hostApprovals: "ask" }, undefined, "ask"],
    ["a saved allow is kept", true, { launchProfile: "plain", hostApprovals: "allow_all" }, undefined, "allow_all"],
    ["a tab launched before profiles were recorded still gets the control", true, null, undefined, "allow_all"],
    ["confinement owns the answer", true, { launchProfile: "plain", hostApprovals: "locked" }, undefined, undefined],
    ["no host approvals means no control", false, { launchProfile: "plain" }, undefined, undefined],
    ["an advertised approve-all mode is the single switch", true, { launchProfile: "native" }, advertised, undefined],
    ["a declared mode the session has not reported yet stays the switch", true, { launchProfile: "native" }, undefined, undefined],
    ["a declared mode the agent does not offer falls back to the host", true, { launchProfile: "native", hostApprovals: "ask" }, others, "ask"],
    ["a loaded session advertising no modes falls back to the host", true, { launchProfile: "native" }, {}, "allow_all"],
    ["modes without a declared approve-all leave the host control", true, { launchProfile: "plain" }, others, "allow_all"],
  ] as const)("%s", (_name, hostApprovals, config, controls, expected) => {
    expect(hostApprovalsChoice(descriptor(hostApprovals), config, controls)).toBe(expected);
  });

  it("is absent without a descriptor", () => {
    expect(hostApprovalsChoice(undefined, { launchProfile: "plain" }, undefined)).toBeUndefined();
  });

  const modeOption = (currentValue: string) => ({ configOptions: [{ id: "opaque-mode", name: "Mode", category: "mode", currentValue, options: [{ value: "default", name: "Default" }, { value: "approve-all", name: "Approve all" }] }] }) as RuntimeSettings;

  it("treats an approve-all mode offered as a config option as the single switch", () => {
    expect(hostApprovalsChoice(descriptor(true), { launchProfile: "native" }, modeOption("default"))).toBeUndefined();
  });

  it.each([
    ["the host answers while the mode row reads otherwise", { launchProfile: "native", hostApprovals: "allow_all" }, advertised, true],
    ["the same through a mode config option", { launchProfile: "native", hostApprovals: "allow_all" }, modeOption("default"), true],
    ["the owner chose an asking mode", { launchProfile: "native", hostApprovals: "ask", sessionMode: "default" }, advertised, false],
    ["the agent really is in its approve-all mode", { launchProfile: "native", hostApprovals: "allow_all" }, { ...advertised, mode: "approve-all" }, false],
    ["the option really is approve-all", { launchProfile: "native", hostApprovals: "allow_all" }, modeOption("approve-all"), false],
    ["the option is approve-all while the session mode lags", { launchProfile: "native", hostApprovals: "allow_all" }, { ...modeOption("approve-all"), mode: "default" }, false],
    ["confinement owns the answer", { launchProfile: "native", hostApprovals: "locked" }, advertised, false],
    ["the Permissions row already says so", { launchProfile: "plain", hostApprovals: "allow_all" }, others, false],
    ["no session has reported yet", { launchProfile: "native", hostApprovals: "allow_all" }, undefined, false],
  ] as const)("hostFullAccessOverride: %s", (_name, config, controls, expected) => {
    expect(hostFullAccessOverride(descriptor(true), config, controls)).toBe(expected);
  });

  it("hostFullAccessOverride needs host approvals", () => {
    expect(hostFullAccessOverride(descriptor(false), { launchProfile: "native", hostApprovals: "allow_all" }, advertised)).toBe(false);
  });
});

describe("the host full-access notice", () => {
  it("adds a read-only Permissions row beside the mode row", () => {
    const sections = pickerSections({ controls: { mode: "default", modes: [{ id: "default", name: "Default" }] }, hostFullAccess: true });
    expect(sections.map(section => section.label)).toEqual(["Mode", "Permissions"]);
    expect(sections[1]).toMatchObject({ current: "Full access (host)", options: [] });
  });
});
