import "./ChatComposer.composition.harness";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { RuntimeSettings } from "@codeterm/plugin-sdk";
import type { AgentRuntimeState } from "../../lib/agentRuntimeState";
import type { ProviderDescriptor, Tab } from "../../lib/types";
import { __setTabModelApiForTest } from "../Sidebar/tabModelApi";
import { StatusLineModelPicker } from "./StatusLineModelPicker";

const bridge = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("../../lib/bridge", () => ({ invoke: bridge.invoke }));
vi.mock("../../store/multiplexer/hooks", () => ({ useMuxSelector: (select: (state: unknown) => unknown) => select({ tabs: { acp: { session_binding_nonce: "acp-nonce", agent_session_id: "acp-session" } } }) }));

const controls: RuntimeSettings = {
  configOptions: [
    { id: "model", name: "Model", category: "model", currentValue: "gemini-pro", options: [{ value: "gemini-pro", name: "Gemini Pro" }, { value: "gemini-flash", name: "Gemini Flash" }] },
    { id: "reasoning_effort", name: "Thinking", category: "thought_level", currentValue: "low", options: [{ value: "low", name: "Low" }, { value: "high", name: "High" }] },
    { id: "readonly", name: "Reported value", currentValue: "opaque", options: [] },
    { id: "approvals", name: "Approvals", category: "mode", currentValue: "ask", options: [{ value: "ask", name: "Ask" }, { value: "auto", name: "Auto" }] },
  ],
  mode: "code",
  modes: [{ id: "code", name: "Code" }, { id: "plan", name: "Plan" }],
};
const state: AgentRuntimeState = { state: "loaded", epoch: 7, agentName: "Gemini CLI", binaryVersion: "0.9.0", controls } as AgentRuntimeState;
const provider = { id: "acp-gemini", display_name: "Gemini CLI", model_switch: "live", available_models: [], agent_runtime: {} } as unknown as ProviderDescriptor;
const tab = { id: "acp", agent_session_id: "acp-session", session_binding_nonce: "acp-nonce", agent_runtime_state: state } as unknown as Tab;

let root: ReturnType<typeof createRoot> | null = null;
let host: HTMLDivElement;
afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  bridge.invoke.mockReset();
  __setTabModelApiForTest(null);
});

async function mount(runtimeControls = controls) {
  bridge.invoke.mockResolvedValue({});
  const switchModel = vi.fn(async () => ({ mode_used: "live", model_confirmation: "pending" }));
  __setTabModelApiForTest({ switchModel } as never);
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root!.render(<>
    <StatusLineModelPicker tab={tab} tabId="acp" descriptor={provider} runtimeControls={runtimeControls} runtimeIdentity={state}
      facts={{ hasSession: true }} label="Gemini CLI" modelCandidates={[]} currentEffort={undefined} />
  </>));
  return switchModel;
}

const row = () => document.body.querySelector('[data-testid="status-model-picker"]')!;
const trigger = () => host.querySelector<HTMLButtonElement>('[data-testid="status-model-trigger"]')!;

describe("declared model controls of a runtime agent", () => {
  it("shows one model control, keeps the other declared controls, and applies through the runtime controls API", async () => {
    const switchModel = await mount();
    expect(host.querySelector('button[aria-label^="Approvals:"]')).toBeNull();
    expect(host.querySelectorAll('[data-testid="status-model-trigger"]')).toHaveLength(1);
    // The same presentation as every other provider: logo, model · effort; the agent's name and version only in the tooltip.
    expect(trigger().textContent).toBe("Gemini Pro · low");
    expect(host.querySelector('[data-testid="status-harness-glyph"]')?.getAttribute("title")).toBe("Gemini CLI · 0.9.0");

    await act(async () => trigger().click());
    const approvals = row().querySelector<HTMLButtonElement>('button[aria-label="Approvals: Ask"]')!;
    expect(approvals).not.toBeNull();
    expect(approvals.disabled).toBe(false);
    expect(approvals.tabIndex).toBe(0);
    approvals.focus();
    expect(document.activeElement).toBe(approvals);
    expect(row().querySelector('output[aria-label="Reported value"]')?.textContent).toBe("opaque");
    expect(row().querySelector('button[aria-label^="Mode:"]')).toBeNull();
    const option = [...document.body.querySelectorAll<HTMLElement>('[role="option"]')].find((node) => node.textContent?.startsWith("Gemini Flash"))!;
    await act(async () => option.click());
    const high = document.body.querySelector<HTMLElement>('[aria-label="Reasoning"] [role="tab"][data-value="high"]')!;
    await act(async () => high.click());
    const apply = [...document.body.querySelectorAll<HTMLButtonElement>("button")].find((button) => button.textContent === "Apply")!;
    await act(async () => apply.click());

    const target = { threadId: "acp-session", bindingNonce: "acp-nonce", epoch: 7, mode: false };
    expect(bridge.invoke.mock.calls).toEqual([
      ["agent_runtime_controls", { tabId: "acp", request: { ...target, id: "model", value: "gemini-flash" } }],
      ["agent_runtime_controls", { tabId: "acp", request: { ...target, id: "reasoning_effort", value: "high" } }],
    ]);
    expect(switchModel).not.toHaveBeenCalled();
  });

  it("a non-model control in the picker still changes through the same API", async () => {
    await mount();
    await act(async () => trigger().click());
    const approvals = row().querySelector<HTMLButtonElement>('button[aria-label^="Approvals:"]')!;
    await act(async () => { approvals.dispatchEvent(new Event("pointerdown", { bubbles: true, cancelable: true })); });
    await act(async () => [...document.body.querySelectorAll<HTMLElement>('[role="menuitemradio"]')].find((item) => item.textContent?.trim() === "Auto")!.click());
    expect(bridge.invoke).toHaveBeenCalledWith("agent_runtime_controls", { tabId: "acp", request: { threadId: "acp-session", bindingNonce: "acp-nonce", epoch: 7, id: "approvals", value: "auto", mode: false } });
  });
});

it("a tab puts extra controls inside the model picker and leaves only its bubble in the composer", async () => {
  const { setAgentDescriptors } = await import("../../lib/agentDescriptorCache");
  const { TabStatusLineModelPicker } = await import("./StatusLineModelPicker");
  setAgentDescriptors([provider]);
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root!.render(<TabStatusLineModelPicker tab={{ ...tab, agent_type: provider.id, backing: { kind: "chat_backend" } } as Tab} label="Gemini CLI" />));
  expect(trigger()).not.toBeNull();
  expect(host.querySelectorAll("button")).toHaveLength(1);
  await act(async () => trigger().click());
  expect(row().querySelector('button[aria-label="Approvals: Ask"]')).not.toBeNull();
});

describe("the Permissions control", () => {
  const launchProfiles = {
    "host-only": { binary: "opaque", args: [] },
    "approve-mode": { binary: "opaque", args: [], fullAccessMode: "yolo" },
  };
  const hostProvider = { ...provider, agent_runtime: { hostApprovals: true, launchProfiles } } as unknown as ProviderDescriptor;
  const modelOnly: RuntimeSettings = { configOptions: controls.configOptions!.filter(option => option.category === "model") };
  const withModes: RuntimeSettings = { ...modelOnly, mode: "default", modes: [{ id: "default", name: "Default" }, { id: "yolo", name: "YOLO" }] };

  async function open(descriptor: ProviderDescriptor, runtimeControls: RuntimeSettings, config: Record<string, unknown>) {
    bridge.invoke.mockResolvedValue({});
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
    await act(async () => root!.render(<StatusLineModelPicker tab={{ ...tab, agent_runtime_config: config } as Tab} tabId="acp" descriptor={descriptor}
      runtimeControls={runtimeControls} runtimeIdentity={state} facts={{ hasSession: true }} label="Agent" modelCandidates={[]} currentEffort={undefined} />));
    await act(async () => trigger().click());
  }
  const permissions = () => row().querySelector<HTMLButtonElement>('button[aria-label^="Permissions:"]');

  it("shows no-questions by default on a host-approval profile and switches live", async () => {
    await open(hostProvider, modelOnly, { launchProfile: "host-only" });
    expect(permissions()?.getAttribute("aria-label")).toBe("Permissions: Allow all");
    await act(async () => permissions()!.dispatchEvent(new Event("pointerdown", { bubbles: true, cancelable: true })));
    await act(async () => [...document.body.querySelectorAll<HTMLElement>('[role="menuitemradio"]')].find(item => item.textContent?.trim() === "Ask each time")!.click());
    expect(bridge.invoke).toHaveBeenCalledWith("agent_runtime_host_approvals", { tabId: "acp", request: { allowAll: false } });
    expect([...document.body.querySelectorAll("button")].some(button => button.textContent === "Apply" && !button.disabled)).toBe(false);
  });

  it("shows the owner's saved choice", async () => {
    await open(hostProvider, modelOnly, { launchProfile: "host-only", hostApprovals: "ask" });
    expect(permissions()?.getAttribute("aria-label")).toBe("Permissions: Ask each time");
  });

  it("leaves the agent's advertised approve-all mode as the single switch", async () => {
    await open(hostProvider, withModes, { launchProfile: "approve-mode" });
    expect(permissions()).toBeNull();
    expect(row().querySelector('button[aria-label="Mode: Default"]')).not.toBeNull();
  });

  it("falls back to the host control when the agent does not offer its declared mode", async () => {
    await open(hostProvider, { ...modelOnly, mode: "default", modes: [{ id: "default", name: "Default" }] }, { launchProfile: "approve-mode" });
    expect(permissions()).not.toBeNull();
  });

  it("is absent on a confined tab and on a profile without host approvals", async () => {
    await open(hostProvider, modelOnly, { launchProfile: "host-only", hostApprovals: "locked" });
    expect(permissions()).toBeNull();
    await act(async () => root?.unmount());
    host.remove();
    await open(provider, modelOnly, {});
    expect(permissions()).toBeNull();
  });

  it("reports a refused switch instead of pretending it applied", async () => {
    await open(hostProvider, modelOnly, { launchProfile: "host-only" });
    bridge.invoke.mockRejectedValue(new Error("Refused"));
    await act(async () => permissions()!.dispatchEvent(new Event("pointerdown", { bubbles: true, cancelable: true })));
    await act(async () => [...document.body.querySelectorAll<HTMLElement>('[role="menuitemradio"]')].find(item => item.textContent?.trim() === "Ask each time")!.click());
    expect(row().textContent).toContain("The agent refused this control value.");
    expect(permissions()?.getAttribute("aria-label")).toBe("Permissions: Allow all");
  });
});

it("legacy mode keeps its pinned mode request and refused settings remain visible", async () => {
  await mount({ ...controls, configOptions: controls.configOptions!.filter(option => option.category !== "mode") });
  await act(async () => trigger().click());
  const mode = row().querySelector<HTMLButtonElement>('button[aria-label="Mode: Code"]')!;
  await act(async () => mode.dispatchEvent(new Event("pointerdown", { bubbles: true, cancelable: true })));
  await act(async () => [...document.body.querySelectorAll<HTMLElement>('[role="menuitemradio"]')].find(item => item.textContent?.trim() === "Plan")!.click());
  expect(bridge.invoke).toHaveBeenCalledWith("agent_runtime_controls", { tabId: "acp", request: { threadId: "acp-session", bindingNonce: "acp-nonce", epoch: 7, id: "", value: "plan", mode: true } });
  bridge.invoke.mockRejectedValue(new Error("Refused"));
  await act(async () => mode.dispatchEvent(new Event("pointerdown", { bubbles: true, cancelable: true })));
  await act(async () => [...document.body.querySelectorAll<HTMLElement>('[role="menuitemradio"]')].find(item => item.textContent?.trim() === "Plan")!.click());
  expect(row().textContent).toContain("The agent refused this control value.");
});
