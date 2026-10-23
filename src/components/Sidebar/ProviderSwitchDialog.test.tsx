import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { ProviderSwitchDialog } from "./ProviderSwitchDialog";
import { __setTabProviderApiForTest } from "./tabProviderApi";
import type { Tab } from "../../lib/types";
import type { TabActivity } from "../../lib/tabActivity";
import type { ProviderSwitchResult } from "./tabProviderMenu";
import { __setInvokeForTest } from "../../lib/bridge";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const state = vi.hoisted(() => ({ status: "Idle", launching: false }));
vi.mock("../../lib/useTabActivity", () => ({ useTabActivity: () => state as unknown as TabActivity }));
vi.mock("../../lib/useAgentDescriptor", () => ({ useAgentDescriptors: () => [
  { id: "source", display_name: "Source", transport: "pty", available_models: [] },
  { id: "unavailable", display_name: "Unavailable target", transport: "pty", available_models: [], availability: "unavailable" },
  { id: "target", display_name: "Target", transport: "chat_backend", agent_runtime: {}, default_model: "model-token", available_models: [
    { id: "model-token", display_name: "Sonnet 5.5", reasoning_efforts: [{ id: "effort-token", display_name: "Effort" }] },
    { id: "haiku-token", display_name: "Haiku 5.5" },
  ] },
] }));

const tab = { id: "owned", agent_type: "source", title: "Baton" } as Tab;
const result: ProviderSwitchResult = { tab_id: "owned", provider: "target", session_id: "new-thread", previous_provider: "source", previous_session_id: "old-thread", previous_history_path: "archive", binding_restored: true, handoff_delivered: true };
afterEach(() => { __setTabProviderApiForTest(null); __setInvokeForTest(null); state.status = "Idle"; });

function button(label: string) {
  const button = Array.from(document.querySelectorAll<HTMLButtonElement>('button')).find((button) => button.textContent === label);
  if (!button) throw new Error(`Missing button ${label}`);
  return button;
}

it("confirmation sends one target selection and shows its result", async () => {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  let finish!: (result: ProviderSwitchResult) => void;
  const pending = new Promise<ProviderSwitchResult>((resolve) => { finish = resolve; });
  const send = vi.fn(() => pending);
  __setTabProviderApiForTest({ switchProvider: send });
  try {
    await act(async () => root.render(<ProviderSwitchDialog tab={tab} initialProviderId="target" onClose={() => {}} />));
    expect(document.querySelector('[role="dialog"]')).not.toBeNull();
    expect(document.querySelector('[aria-label="Target provider: Target"]')).not.toBeNull();
    expect(document.querySelector('[aria-label="Target model: Sonnet 5.5"]')).not.toBeNull();
    expect(document.querySelector('[aria-label="Reasoning effort: Effort"]')).not.toBeNull();
    expect(send).not.toHaveBeenCalled();
    await act(async () => { const confirm = button("Switch provider"); confirm.click(); confirm.click(); });
    expect(send).toHaveBeenCalledExactlyOnceWith("owned", "target", { model: "model-token", reasoningEffort: "effort-token", force: false });
    await act(async () => finish(result));
    expect(button("Previous provider: source")).not.toBeNull();
    __setInvokeForTest(async <T,>(command: string) => {
      expect(command).toBe("read_provider_history");
      return [{ type: "user", content: "opaque-prior-🪵" }] as T;
    });
    await act(async () => button("Previous provider: source").click());
    expect(document.querySelector('[aria-label="Previous provider transcript"]')?.textContent).toContain("opaque-prior-🪵");
  } finally { await act(async () => root.unmount()); host.remove(); }
});

it("exposes the selected value and menu state, and changes selection by keyboard", async () => {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<ProviderSwitchDialog tab={tab} initialProviderId="target" onClose={() => {}} />));
    const trigger = document.querySelector<HTMLButtonElement>('button[aria-label="Target model: Sonnet 5.5"]');
    expect(trigger).not.toBeNull();
    expect(trigger?.getAttribute("aria-haspopup")).toBe("menu");
    expect(trigger?.getAttribute("aria-expanded")).toBe("false");
    const menuId = trigger?.getAttribute("aria-controls");
    expect(menuId).toBeTruthy();
    expect(document.getElementById(menuId!)?.hidden).toBe(true);

    await act(async () => trigger?.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true, cancelable: true })));
    expect(trigger?.getAttribute("aria-expanded")).toBe("true");
    const menu = document.getElementById(menuId!);
    expect(menu?.getAttribute("role")).toBe("menu");
    const sonnet = Array.from(menu?.querySelectorAll<HTMLButtonElement>('[role="menuitemradio"]') ?? [])
      .find((item) => item.textContent?.includes("Sonnet 5.5"));
    const haiku = Array.from(menu?.querySelectorAll<HTMLButtonElement>('[role="menuitemradio"]') ?? [])
      .find((item) => item.textContent?.includes("Haiku 5.5"));
    expect(sonnet?.getAttribute("aria-checked")).toBe("true");
    expect(haiku?.getAttribute("aria-checked")).toBe("false");
    expect(document.activeElement).toBe(sonnet);

    await act(async () => {
      sonnet?.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true, cancelable: true }));
      expect(document.activeElement).toBe(haiku);
      haiku?.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true }));
    });
    const updatedTrigger = document.querySelector<HTMLButtonElement>('button[aria-label="Target model: Haiku 5.5"]');
    expect(updatedTrigger?.getAttribute("aria-expanded")).toBe("false");
    expect(document.activeElement).toBe(updatedTrigger);

    await act(async () => updatedTrigger?.dispatchEvent(new KeyboardEvent("keydown", { key: " ", bubbles: true, cancelable: true })));
    expect(updatedTrigger?.getAttribute("aria-expanded")).toBe("true");
    await act(async () => document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true })));
    expect(updatedTrigger?.getAttribute("aria-expanded")).toBe("false");
    expect(document.activeElement).toBe(updatedTrigger);

    await act(async () => updatedTrigger?.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true, cancelable: true })));
    const selectedHaiku = Array.from(document.querySelectorAll<HTMLButtonElement>('[role="menuitemradio"]'))
      .find((item) => item.textContent?.includes("Haiku 5.5"));
    expect(selectedHaiku?.getAttribute("aria-checked")).toBe("true");
    await act(async () => selectedHaiku?.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true })));
    expect(updatedTrigger?.getAttribute("aria-expanded")).toBe("false");
    expect(document.activeElement).toBe(updatedTrigger);
  } finally { await act(async () => root.unmount()); host.remove(); }
});

it("waiting follows the idle event and never submits while working", async () => {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  const send = vi.fn(async () => result);
  __setTabProviderApiForTest({ switchProvider: send });
  state.status = "Working";
  const render = () => root.render(<ProviderSwitchDialog tab={tab} initialProviderId="target" onClose={() => {}} />);
  try {
    await act(async () => render());
    await act(async () => button("Wait for idle").click());
    expect(send).not.toHaveBeenCalled();
    state.status = "Idle";
    await act(async () => render());
    expect(send).toHaveBeenCalledTimes(1);
  } finally { await act(async () => root.unmount()); host.remove(); }
});

it("explicit stop is carried and refusals stay visible without retry", async () => {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  const send = vi.fn(async () => { throw new Error("opaque-refusal"); });
  __setTabProviderApiForTest({ switchProvider: send });
  state.status = "Working";
  try {
    await act(async () => root.render(<ProviderSwitchDialog tab={tab} initialProviderId="target" onClose={() => {}} />));
    await act(async () => button("Stop and switch").click());
    expect(send).toHaveBeenCalledExactlyOnceWith("owned", "target", { model: "model-token", reasoningEffort: "effort-token", force: true });
    expect(document.querySelector('[role="status"]')?.textContent).toContain("opaque-refusal");
  } finally { await act(async () => root.unmount()); host.remove(); }
});

 it("accepted operations wait for daemon completion before showing success", async () => {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  let finish!: (value: unknown) => void;
  __setTabProviderApiForTest({ switchProvider: async () => ({ operation_id: "operation-token", tab_id: "owned", provider: "target", phase: "pending", outcome: null, error: null }) });
  const read = vi.fn(async (command: string, args?: Record<string, unknown>) => {
    expect(command).toBe("provider_switch_status");
    expect(args).toEqual({ tabId: "owned", operationId: "operation-token", wait: true });
    return new Promise<unknown>((resolve) => { finish = resolve; });
  });
  __setInvokeForTest(async <T,>(command: string, args?: Record<string, unknown>) => await read(command, args) as T);
  try {
    await act(async () => root.render(<ProviderSwitchDialog tab={tab} initialProviderId="target" onClose={() => {}} />));
    await act(async () => button("Switch provider").click());
    expect(read).toHaveBeenCalledTimes(1);
    expect(document.querySelector('[role="status"]')).not.toBeNull();
    expect(document.querySelector('[aria-label="Previous provider transcript"]')).toBeNull();
    await act(async () => finish({ phase: "completed", outcome: result }));
    expect(button("Previous provider: source")).not.toBeNull();
  } finally { await act(async () => root.unmount()); host.remove(); }
});


it("disables unavailable targets with a reason and refuses submission from a stale selection", async () => {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  const send = vi.fn(async () => result);
  __setTabProviderApiForTest({ switchProvider: send });
  try {
    await act(async () => root.render(<ProviderSwitchDialog tab={tab} initialProviderId="unavailable" onClose={() => {}} />));
    expect(button("Switch provider").disabled).toBe(true);
    const trigger = document.querySelector<HTMLButtonElement>('button[aria-label^="Target provider:"]');
    await act(async () => trigger?.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true, cancelable: true })));
    const unavailable = Array.from(document.querySelectorAll<HTMLButtonElement>('[role="menuitemradio"]')).find(item => item.textContent?.includes("Unavailable target"));
    expect(unavailable?.disabled).toBe(true);
    expect(unavailable?.textContent).toContain("not installed");
    await act(async () => { unavailable?.click(); button("Switch provider").click(); });
    expect(send).not.toHaveBeenCalled();
  } finally { await act(async () => root.unmount()); host.remove(); }
});


it("offers an explicit retry after an authentication probe timeout", async () => {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  const send = vi.fn().mockRejectedValueOnce(new Error("provider_switch_auth_timeout: fixture")).mockResolvedValueOnce(result);
  __setTabProviderApiForTest({ switchProvider: send });
  try {
    await act(async () => root.render(<ProviderSwitchDialog tab={tab} initialProviderId="target" onClose={() => {}} />));
    await act(async () => button("Switch provider").click());
    expect(document.querySelector('[aria-label="The provider authentication check timed out"]')).not.toBeNull();
    expect(send).toHaveBeenCalledTimes(1);
    await act(async () => button("Retry").click());
    expect(send).toHaveBeenCalledTimes(2);
    expect(button("Previous provider: source")).not.toBeNull();
  } finally { await act(async () => root.unmount()); host.remove(); }
});
