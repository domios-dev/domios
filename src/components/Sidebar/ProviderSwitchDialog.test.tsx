import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { ProviderSwitchDialog } from "./ProviderSwitchDialog";
import { __setTabProviderApiForTest } from "./tabProviderApi";
import type { Tab } from "../../lib/types";
import type { TabActivity } from "../../lib/tabActivity";
import type { ProviderSwitchResult, ProviderSwitchStatus } from "./tabProviderMenu";
import { __setInvokeForTest, __setListenForTest } from "../../lib/bridge";
import { providerSwitchStatusView } from "./providerSwitchStatusModel";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const state = vi.hoisted(() => ({ status: "Idle", launching: false }));
const navigation = vi.hoisted(() => ({ show: vi.fn(), activeTabId: () => null }));
vi.mock("../../lib/useTabActivity", () => ({ useTabActivity: () => state as unknown as TabActivity }));
vi.mock("../AgentPicker/pickerNavigation", () => ({ usePickerNavigation: () => navigation }));
vi.mock("../../lib/useAgentDescriptor", () => ({ useAgentDescriptor: () => undefined, useAgentDescriptors: () => [
  { id: "source", display_name: "Source", transport: "pty", available_models: [] },
  { id: "unavailable", display_name: "Unavailable target", transport: "pty", available_models: [], availability: "unavailable", install_command: "opaque-install" },
  { id: "target", display_name: "Target", transport: "chat_backend", agent_runtime: {}, default_model: "model-token", available_models: [
    { id: "model-token", display_name: "Sonnet 5.5", reasoning_efforts: [{ id: "effort-token", display_name: "Effort" }] },
    { id: "haiku-token", display_name: "Haiku 5.5" },
  ] },
] }));

const tab = { id: "owned", agent_type: "source", title: "Baton" } as Tab;
const result: ProviderSwitchResult = { tab_id: "owned", provider: "target", session_id: "new-thread", previous_provider: "source", previous_session_id: "old-thread", previous_history_path: "archive", binding_restored: true, handoff_delivered: true };
const acceptedStatus: ProviderSwitchStatus = { operation_id: "operation-token", tab_id: "owned", provider: "target", phase: "pending", outcome: null, error: null };
let publishStatus: (status: ProviderSwitchStatus) => void = () => {};
beforeEach(() => {
  __setListenForTest(async <T,>(event: string, handler: (value: { payload: T }) => void) => {
    expect(event).toBe("provider-switch-status");
    publishStatus = (status) => handler({ payload: status as T });
    return () => {};
  });
});
afterEach(() => { __setTabProviderApiForTest(null); __setInvokeForTest(null); __setListenForTest(null); publishStatus = () => {}; state.status = "Idle"; navigation.show.mockReset(); vi.useRealTimers(); });

function button(label: string) {
  const button = Array.from(document.querySelectorAll<HTMLButtonElement>('button')).find((button) => button.textContent === label);
  if (!button) throw new Error(`Missing button ${label}`);
  return button;
}

it("maps pending status to target startup feedback", () => {
  expect(providerSwitchStatusView({ phase: "pending", error: null }, "Codex"))
    .toEqual({ kind: "busy", label: "Starting Codex…" });
});

it("maps running status to context handoff feedback", () => {
  expect(providerSwitchStatusView({ phase: "running", error: null }, "Codex"))
    .toEqual({ kind: "busy", label: "Handing over context…" });
});

it("maps completed status to a bound confirmation", () => {
  expect(providerSwitchStatusView({ phase: "completed", error: null }, "Codex"))
    .toEqual({ kind: "complete", label: "Bound — continuing" });
});

it("preserves daemon failure reasons in the status view", () => {
  expect(providerSwitchStatusView({ phase: "failed", error: "provider_switch_auth_timeout: fixture" }, "Codex"))
    .toEqual({ kind: "failed", reason: "provider_switch_auth_timeout: fixture" });
});

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
    expect(document.querySelector('[aria-label="Agent provider: Target"]')).not.toBeNull();
    expect(document.querySelector('[aria-label="Agent model: Sonnet 5.5"]')).not.toBeNull();
    expect(document.querySelector('[aria-label="Agent effort: Effort"]')).not.toBeNull();
    expect(send).not.toHaveBeenCalled();
    await act(async () => { const confirm = button("Switch harness"); confirm.click(); confirm.click(); });
    expect(send).toHaveBeenCalledExactlyOnceWith("owned", "target", { model: "model-token", reasoningEffort: "effort-token", force: false });
    await act(async () => finish(result));
    expect(button("Previous harness: source")).not.toBeNull();
    __setInvokeForTest(async <T,>(command: string) => {
      expect(command).toBe("read_provider_history");
      return [{ type: "user", content: "opaque-prior-🪵" }] as T;
    });
    await act(async () => button("Previous harness: source").click());
    expect(document.querySelector('[aria-label="Previous harness transcript"]')?.textContent).toContain("opaque-prior-🪵");
  } finally { await act(async () => root.unmount()); host.remove(); }
});

it("exposes the selected value and menu state, and changes selection by keyboard", async () => {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<ProviderSwitchDialog tab={tab} initialProviderId="target" onClose={() => {}} />));
    const trigger = document.querySelector<HTMLButtonElement>('button[aria-label="Agent model: Sonnet 5.5"]');
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
    expect(menu?.closest('[aria-modal="true"]')).toBe(document.querySelector('[role="dialog"]'));
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
    const updatedTrigger = document.querySelector<HTMLButtonElement>('button[aria-label="Agent model: Haiku 5.5"]');
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
    for (const name of ["Agent provider: Target", "Agent model: Sonnet 5.5", "Agent effort: Effort"]) {
      expect(document.querySelector<HTMLButtonElement>(`button[aria-label="${name}"]`)?.disabled).toBe(true);
    }
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

it("follows subscribed operation phases and buffers status that arrives before acceptance", async () => {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  let accept!: (status: ProviderSwitchStatus) => void;
  const pending = new Promise<ProviderSwitchStatus>((resolve) => { accept = resolve; });
  const send = vi.fn(() => pending);
  __setTabProviderApiForTest({ switchProvider: send });
  const invoke = vi.fn();
  __setInvokeForTest(async <T,>(command: string) => { invoke(command); return undefined as T; });
  try {
    await act(async () => root.render(<ProviderSwitchDialog tab={tab} initialProviderId="target" onClose={() => {}} />));
    await act(async () => button("Switch harness").click());
    expect(send).toHaveBeenCalledTimes(1);
    expect(document.querySelector('[role="status"]')?.textContent).toContain("Starting Target…");
    expect(document.querySelector('[role="status"][aria-busy="true"]')).not.toBeNull();
    expect(document.querySelector('[aria-label="Previous harness transcript"]')).toBeNull();
    await act(async () => publishStatus({ ...acceptedStatus, phase: "running" }));
    await act(async () => accept(acceptedStatus));
    expect(document.querySelector('[role="status"]')?.textContent).toContain("Handing over context…");
    await act(async () => publishStatus({ ...acceptedStatus, phase: "completed", outcome: result }));
    expect(document.querySelector('[role="status"]')?.textContent).toContain("Bound — continuing");
    expect(document.querySelector('[role="status"][aria-busy="true"]')).toBeNull();
    expect(button("Previous harness: source")).not.toBeNull();
    expect(invoke).not.toHaveBeenCalled();
  } finally { await act(async () => root.unmount()); host.remove(); }
});

it("shows daemon failure details and keeps the existing retry path", async () => {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  const send = vi.fn().mockResolvedValueOnce(acceptedStatus).mockResolvedValueOnce(result);
  __setTabProviderApiForTest({ switchProvider: send });
  try {
    await act(async () => root.render(<ProviderSwitchDialog tab={tab} initialProviderId="target" onClose={() => {}} />));
    await act(async () => button("Switch harness").click());
    await act(async () => publishStatus({ ...acceptedStatus, phase: "failed", error: "provider_switch_auth_timeout: fixture" }));
    expect(document.querySelector('[aria-label="The harness authentication check timed out"]')).not.toBeNull();
    await act(async () => button("Switch harness").click());
    expect(send).toHaveBeenCalledTimes(2);
    expect(button("Previous harness: source")).not.toBeNull();
  } finally { await act(async () => root.unmount()); host.remove(); }
});


it("disables unavailable targets with a reason and refuses submission from a stale selection", async () => {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  const send = vi.fn(async () => result);
  __setTabProviderApiForTest({ switchProvider: send });
  try {
    await act(async () => root.render(<ProviderSwitchDialog tab={tab} initialProviderId="unavailable" onClose={() => {}} />));
    expect(button("Switch harness").disabled).toBe(true);
    const trigger = document.querySelector<HTMLButtonElement>('button[aria-label^="Agent provider:"]');
    await act(async () => trigger?.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true, cancelable: true })));
    const unavailable = Array.from(document.querySelectorAll<HTMLButtonElement>('[role="menuitemradio"]')).find(item => item.textContent?.includes("Unavailable target"));
    expect(unavailable?.disabled).toBe(true);
    expect(unavailable?.textContent).toContain("not installed");
    await act(async () => { unavailable?.click(); button("Switch harness").click(); });
    expect(send).not.toHaveBeenCalled();
  } finally { await act(async () => root.unmount()); host.remove(); }
});

it("installs an unavailable target in its terminal tab and leaves the dialog", async () => {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  const onClose = vi.fn();
  const install = vi.fn(async (_args?: Record<string, unknown>) => ({ tab_id: "installer-tab", workspace_id: "installer-workspace", launch_state: "command_dispatched" }));
  __setInvokeForTest(async <T,>(command: string, args?: Record<string, unknown>) => {
    expect(command).toBe("install_provider_in_tab");
    return await install(args) as T;
  });
  try {
    await act(async () => root.render(<ProviderSwitchDialog tab={tab} initialProviderId="unavailable" onClose={onClose} />));
    await act(async () => button("Install in terminal").click());
    expect(install).toHaveBeenCalledExactlyOnceWith({ providerId: "unavailable" });
    expect(onClose).toHaveBeenCalledTimes(1);
    expect(navigation.show).toHaveBeenCalledExactlyOnceWith({ workspaceId: "installer-workspace", tabId: "installer-tab" });
  } finally { await act(async () => root.unmount()); host.remove(); }
});


it("offers an explicit retry after an authentication probe timeout", async () => {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  const send = vi.fn().mockRejectedValueOnce(new Error("provider_switch_auth_timeout: fixture")).mockResolvedValueOnce(result);
  __setTabProviderApiForTest({ switchProvider: send });
  try {
    await act(async () => root.render(<ProviderSwitchDialog tab={tab} initialProviderId="target" onClose={() => {}} />));
    await act(async () => button("Switch harness").click());
    expect(document.querySelector('[aria-label="The harness authentication check timed out"]')).not.toBeNull();
    expect(send).toHaveBeenCalledTimes(1);
    await act(async () => button("Switch harness").click());
    expect(send).toHaveBeenCalledTimes(2);
    expect(button("Previous harness: source")).not.toBeNull();
  } finally { await act(async () => root.unmount()); host.remove(); }
});

it("keeps technical details collapsed and copies them without the Error prefix", async () => {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  __setTabProviderApiForTest({ switchProvider: async () => { throw new Error("Runtime transcript binding is not current"); } });
  const writeText = vi.fn(async () => {});
  vi.stubGlobal("navigator", { clipboard: { writeText } });
  try {
    await act(async () => root.render(<ProviderSwitchDialog tab={tab} initialProviderId="target" onClose={() => {}} />));
    await act(async () => button("Switch harness").click());
    const disclosure = button("Details");
    const picker = document.querySelector('[aria-label="Agent provider: Target"]');
    const sentence = document.getElementById(picker?.getAttribute("aria-describedby") ?? "");
    expect(sentence).not.toBeNull();
    expect(sentence?.closest('[aria-live="polite"]')).not.toBeNull();
    const detail = document.getElementById(disclosure.getAttribute("aria-controls")!);
    expect(disclosure.getAttribute("aria-expanded")).toBe("false");
    expect(detail?.hidden).toBe(true);
    expect(document.querySelector('[role="status"] p')?.textContent).not.toContain("Runtime transcript");
    await act(async () => disclosure.click());
    expect(detail?.hidden).toBe(false);
    await act(async () => button("Copy").click());
    expect(writeText).toHaveBeenCalledExactlyOnceWith("Runtime transcript binding is not current");
  } finally { await act(async () => root.unmount()); host.remove(); vi.unstubAllGlobals(); }
});

it("names the selected harness in an installation refusal", async () => {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  __setTabProviderApiForTest({ switchProvider: async () => { throw new Error("provider_switch_unavailable: fixture executable missing"); } });
  try {
    await act(async () => root.render(<ProviderSwitchDialog tab={tab} initialProviderId="target" onClose={() => {}} />));
    await act(async () => button("Switch harness").click());
    const notice = document.querySelector('[role="status"]');
    expect(notice?.textContent).toContain("Target isn't installed on this machine");
    expect(notice?.querySelector("pre")?.hidden).toBe(true);
  } finally { await act(async () => root.unmount()); host.remove(); }
});

it("disables a known cooldown until its deadline without retrying the switch", async () => {
  vi.useFakeTimers();
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  const send = vi.fn().mockRejectedValueOnce(new Error("provider_switch_rate_limited: 19")).mockResolvedValueOnce(result);
  __setTabProviderApiForTest({ switchProvider: send });
  try {
    await act(async () => root.render(<ProviderSwitchDialog tab={tab} initialProviderId="target" onClose={() => {}} />));
    await act(async () => button("Switch harness").click());
    expect(button("Switch in 19 s").disabled).toBe(true);
    await act(async () => { vi.advanceTimersByTime(18999); button("Switch in 19 s").click(); });
    expect(send).toHaveBeenCalledTimes(1);
    await act(async () => vi.advanceTimersByTime(1));
    expect(button("Switch harness").disabled).toBe(false);
    expect(send).toHaveBeenCalledTimes(1);
    await act(async () => button("Switch harness").click());
    expect(send).toHaveBeenCalledTimes(2);
  } finally { await act(async () => root.unmount()); host.remove(); }
});
