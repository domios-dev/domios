import "./ChatComposer.composition.harness";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { RuntimeSettings } from "@codeterm/plugin-sdk";
import type { AgentRuntimeState } from "../../lib/agentRuntimeState";
import type { ProviderDescriptor, Tab } from "../../lib/types";
import { pickerControls, rowControls } from "../../lib/runtimeDeclaredControls";
import { __setTabModelApiForTest } from "../Sidebar/tabModelApi";
import { RuntimeDeclaredControls } from "./RuntimeDeclaredControls";
import { StatusLineModelPicker } from "./StatusLineModelPicker";

const bridge = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("../../lib/bridge", () => ({ invoke: bridge.invoke }));
vi.mock("../../store/multiplexer/hooks", () => ({ useMuxSelector: (select: (state: unknown) => unknown) => select({ tabs: { acp: { session_binding_nonce: "acp-nonce", agent_session_id: "acp-session" } } }) }));

const controls: RuntimeSettings = {
  configOptions: [
    { id: "model", name: "Model", category: "model", currentValue: "gemini-pro", options: [{ value: "gemini-pro", name: "Gemini Pro" }, { value: "gemini-flash", name: "Gemini Flash" }] },
    { id: "reasoning_effort", name: "Thinking", category: "thought_level", currentValue: "low", options: [{ value: "low", name: "Low" }, { value: "high", name: "High" }] },
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

async function mount() {
  bridge.invoke.mockResolvedValue({});
  const switchModel = vi.fn(async () => ({ mode_used: "live", model_confirmation: "pending" }));
  __setTabModelApiForTest({ switchModel } as never);
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root!.render(<>
    <RuntimeDeclaredControls tabId="acp" state={state} />
    <StatusLineModelPicker tab={tab} tabId="acp" descriptor={provider} runtimeControls={controls} runtimeIdentity={state}
      facts={{ hasSession: true }} label="Gemini CLI" modelCandidates={[]} currentEffort={undefined} />
  </>));
  return switchModel;
}

const row = () => host.querySelector('[role="group"][aria-label="Agent controls"]')!;
const trigger = () => host.querySelector<HTMLButtonElement>('[data-testid="status-model-trigger"]')!;

describe("declared model controls of a runtime agent", () => {
  it("the model and thought level belong to the picker by category; other controls stay in the row", () => {
    const owned = pickerControls(controls);
    expect(owned.model?.id).toBe("model");
    expect(owned.effort?.id).toBe("reasoning_effort");
    expect(rowControls(controls).map((option) => option.id)).toEqual(["approvals"]);
    expect(pickerControls({ configOptions: [{ id: "model", name: "Model", currentValue: "x", options: [{ value: "x", name: "X" }] }] }).model).toBeUndefined();
  });

  it("shows one model control, keeps the other declared controls, and applies through the runtime controls API", async () => {
    const switchModel = await mount();
    expect(row().querySelector('[aria-label="Model"]')).toBeNull();
    expect(row().querySelector('[aria-label="Thinking"]')).toBeNull();
    expect(row().querySelector('[role="combobox"][aria-label="Approvals"]')).not.toBeNull();
    expect(row().querySelector('[role="combobox"][aria-label="Session mode"]')).not.toBeNull();
    expect(host.querySelectorAll('[data-testid="status-model-trigger"]')).toHaveLength(1);
    // The same presentation as every other provider: logo, model · effort; the agent's name and version only in the tooltip.
    expect(trigger().textContent).toBe("Gemini Pro · low");
    expect(host.querySelector('[data-testid="status-harness-glyph"]')?.getAttribute("title")).toBe("Gemini CLI · 0.9.0");

    await act(async () => trigger().click());
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

  it("a non-model control in the row still changes through the same API", async () => {
    await mount();
    const approvals = row().querySelector<HTMLSelectElement>('[aria-label="Approvals"]')!;
    await act(async () => { approvals.value = "auto"; approvals.dispatchEvent(new Event("change", { bubbles: true })); });
    expect(bridge.invoke).toHaveBeenCalledWith("agent_runtime_controls", { tabId: "acp", request: { threadId: "acp-session", bindingNonce: "acp-nonce", epoch: 7, id: "approvals", value: "auto", mode: false } });
  });
});
