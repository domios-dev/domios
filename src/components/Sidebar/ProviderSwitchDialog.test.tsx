import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { ProviderSwitchDialog } from "./ProviderSwitchDialog";
import { __setTabProviderApiForTest } from "./tabProviderApi";
import type { Tab } from "../../lib/types";
import type { TabActivity } from "../../lib/tabActivity";
import type { ProviderSwitchResult, ProviderSwitchStatus } from "./tabProviderMenu";
import { __setInvokeForTest, __setListenForTest } from "../../lib/bridge";
import { preferProviderSwitchStatus, providerSwitchStatusView } from "./providerSwitchStatusModel";
import { EVENT_STREAM_LOST, EVENT_STREAM_READY } from "../../lib/eventStreamReady";
import { fireResync } from "../../lib/resync";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const state = vi.hoisted(() => ({ status: "Idle", launching: false }));
const navigation = vi.hoisted(() => ({ show: vi.fn(), activeTabId: () => null }));
const remoteSignals = vi.hoisted(() => ({ handler: null as ((signal: "ready" | "resync" | "lost") => void) | null }));
vi.mock("../../lib/useTabActivity", () => ({ useTabActivity: () => state as unknown as TabActivity }));
vi.mock("../AgentPicker/pickerNavigation", () => ({ usePickerNavigation: () => navigation }));
vi.mock("../../lib/remoteTabEvents", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../lib/remoteTabEvents")>();
  return { ...actual, onRemoteTabEventSignal: (_event: string, handler: (signal: "ready" | "resync" | "lost") => void) => {
    remoteSignals.handler = handler;
    return () => { remoteSignals.handler = null; };
  } };
});
vi.mock("../../lib/useAgentDescriptor", () => ({ useAgentDescriptor: () => undefined, useAgentDescriptors: () => [
  { id: "source", display_name: "Source", transport: "pty", available_models: [] },
  { id: "unavailable", display_name: "Unavailable target", transport: "pty", available_models: [], availability: "unavailable", install_command: "opaque-install" },
  { id: "target", display_name: "Target", transport: "chat_backend", agent_runtime: {}, default_model: "model-token", available_models: [
    { id: "model-token", display_name: "Sonnet 5.5", reasoning_efforts: [{ id: "effort-token", display_name: "Effort" }] },
    { id: "haiku-token", display_name: "Haiku 5.5" },
  ] },
  { id: "grouped", display_name: "Grouped Target", transport: "pty", default_model: "model-token", available_models: [
    { id: "model-token", display_name: "Shared Model", group: "Published models" },
    { id: "other-token", display_name: "Other Model", group: "Published models" },
  ] },
  { id: "other", display_name: "Other", transport: "pty", available_models: [] },
] }));

const tab = { id: "owned", agent_type: "source", title: "Baton" } as Tab;
const remoteTab = { ...tab, id: "mesh:peer/remote" };
const result: ProviderSwitchResult = { tab_id: "owned", provider: "target", session_id: "new-thread", previous_provider: "source", previous_session_id: "old-thread", previous_history_path: "archive", binding_restored: true, handoff_delivered: true };
const acceptedStatus: ProviderSwitchStatus = { operation_id: "operation-token", tab_id: "owned", provider: "target", phase: "pending", outcome: null, error: null };
let publishStatus: (status: ProviderSwitchStatus) => void = () => {};
beforeEach(() => {
  __setListenForTest(async <T,>(event: string, handler: (value: { payload: T }) => void) => {
    expect(event).toMatch(/^provider-switch-status(?::mesh:peer\/remote)?$/);
    publishStatus = (status) => handler({ payload: status as T });
    return () => {};
  });
});
afterEach(() => { __setTabProviderApiForTest(null); __setInvokeForTest(null); __setListenForTest(null); publishStatus = () => {}; remoteSignals.handler = null; state.status = "Idle"; navigation.show.mockReset(); vi.useRealTimers(); vi.unstubAllGlobals(); });

function button(label: string) {
  const button = Array.from(document.querySelectorAll<HTMLButtonElement>('button')).find((button) => button.textContent === label);
  if (!button) throw new Error(`Missing button ${label}`);
  return button;
}

const completedSentence = "Baton now continues on Target · Sonnet 5.5.";

function expectCompletion() {
  const dialog = document.querySelector('[role="dialog"]');
  expect(dialog?.querySelector('[role="status"]')?.textContent).toBe(completedSentence);
  expect(Array.from(dialog?.querySelectorAll("button") ?? [], (entry) => entry.textContent)).toEqual(["Close"]);
}

function mockStatusReads(read: () => ProviderSwitchStatus | Promise<ProviderSwitchStatus>) {
  const request = vi.fn<(command: string, args?: Record<string, unknown>) => ReturnType<typeof read>>(read);
  __setInvokeForTest(async <T,>(command: string, args?: Record<string, unknown>) => await request(command, args) as T);
  return request;
}

it("maps pending status to target startup feedback", () => {
  expect(providerSwitchStatusView({ phase: "pending", error: null }, "Codex", "Baton", "GPT-6.2-Sol"))
    .toEqual({ kind: "busy", label: "Starting Codex…" });
});

it("maps running status to generic switch feedback", () => {
  expect(providerSwitchStatusView({ phase: "running", error: null }, "Codex", "Baton", "GPT-6.2-Sol"))
    .toEqual({ kind: "busy", label: "Switching to Codex…" });
});

it("maps completed status to a bound confirmation", () => {
  expect(providerSwitchStatusView({ phase: "completed", error: null }, "Codex", "Baton", "GPT-6.2-Sol"))
    .toEqual({ kind: "complete", label: "Baton now continues on Codex · GPT-6.2-Sol." });
});

it("preserves daemon failure reasons in the status view", () => {
  expect(providerSwitchStatusView({ phase: "failed", error: "provider_switch_auth_timeout: fixture" }, "Codex", "Baton", "GPT-6.2-Sol"))
    .toEqual({ kind: "failed", reason: "provider_switch_auth_timeout: fixture" });
});

it("keeps phase evidence monotonic and terminal snapshots win over progress", () => {
  const running = { ...acceptedStatus, phase: "running" };
  const completed = { ...acceptedStatus, phase: "completed", outcome: result };
  expect(preferProviderSwitchStatus(running, acceptedStatus)).toBe(running);
  expect(preferProviderSwitchStatus(acceptedStatus, completed)).toBe(completed);
  expect(preferProviderSwitchStatus(completed, running)).toBe(completed);
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
    const dialog = document.querySelector('[role="dialog"]');
    expect(document.getElementById(dialog?.getAttribute("aria-labelledby") ?? "")?.textContent).toBe("Switch provider");
    expect(document.getElementById(dialog?.getAttribute("aria-describedby") ?? "")?.textContent)
      .toBe("Baton continues with another provider, using recent messages and task context. The full history stays here.");
    expect(document.querySelector('[aria-label="Agent provider: Target"]')).not.toBeNull();
    expect(document.querySelector('[aria-label="Agent model: Sonnet 5.5"]')).not.toBeNull();
    expect(document.querySelector('[aria-label="Agent effort: Effort"]')).not.toBeNull();
    expect(send).not.toHaveBeenCalled();
    await act(async () => { const confirm = button("Switch"); confirm.click(); confirm.click(); });
    expect(send).toHaveBeenCalledExactlyOnceWith("owned", "target", { model: "model-token", reasoningEffort: "effort-token", force: false });
    await act(async () => root.render(<ProviderSwitchDialog tab={{ ...tab, agent_type: "target" }} initialProviderId="target" onClose={() => {}} />));
    for (const name of ["Agent provider: Target", "Agent model: Sonnet 5.5", "Agent effort: Effort"]) {
      expect(document.querySelector<HTMLButtonElement>(`button[aria-label="${name}"]`)?.disabled).toBe(true);
    }
    expect(dialog?.textContent).not.toContain("No models published");
    await act(async () => finish(result));
    expectCompletion();
  } finally { await act(async () => root.unmount()); host.remove(); }
});

it("keeps the default selected without marking it current on a different provider", async () => {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  try {
    const runningTab = { ...tab, agent_session_model: "model-token" };
    await act(async () => root.render(<ProviderSwitchDialog tab={runningTab} initialProviderId="grouped" onClose={() => {}} />));
    const trigger = document.querySelector<HTMLButtonElement>('button[aria-label="Agent model: Shared Model"]');
    expect(trigger).not.toBeNull();
    await act(async () => trigger?.click());

    const options = Array.from(document.querySelectorAll('[role="option"]'));
    expect(options.map((option) => option.getAttribute("aria-label"))).toEqual(["Shared Model", "Other Model"]);
    expect(options.find((option) => option.getAttribute("aria-selected") === "true")?.getAttribute("aria-label")).toBe("Shared Model");
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
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

it("keeps matching early terminal evidence when another same-tab operation reports before acceptance", async () => {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  let accept!: (status: ProviderSwitchStatus) => void;
  const pending = new Promise<ProviderSwitchStatus>((resolve) => { accept = resolve; });
  const send = vi.fn(() => pending);
  __setTabProviderApiForTest({ switchProvider: send });
  const readStatus = mockStatusReads(() => acceptedStatus);
  try {
    await act(async () => root.render(<ProviderSwitchDialog tab={tab} initialProviderId="target" onClose={() => {}} />));
    await act(async () => button("Switch").click());
    expect(send).toHaveBeenCalledTimes(1);
    expect(document.querySelector('[role="status"]')).toBeNull();
    const completed = { ...acceptedStatus, phase: "completed", outcome: result };
    await act(async () => publishStatus(completed));
    await act(async () => publishStatus({ ...acceptedStatus, operation_id: "other-operation", phase: "failed", error: "foreign failure" }));
    await act(async () => accept(acceptedStatus));
    expect(document.querySelector('[role="status"]')?.textContent).toBe(completedSentence);
    expect(document.querySelector('[role="status"]')?.textContent).not.toContain("foreign failure");
    expectCompletion();
    expect(readStatus).not.toHaveBeenCalled();
  } finally { await act(async () => root.unmount()); host.remove(); }
});

it("ignores another operation's terminal event in the accessible dialog while the accepted operation is active", async () => {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  const send = vi.fn(async () => acceptedStatus);
  __setTabProviderApiForTest({ switchProvider: send });
  __setInvokeForTest(async <T,>() => acceptedStatus as T);
  try {
    await act(async () => root.render(<ProviderSwitchDialog tab={tab} initialProviderId="target" onClose={() => {}} />));
    await act(async () => button("Switch").click());
    await act(async () => publishStatus({ ...acceptedStatus, operation_id: "other-operation", phase: "completed", outcome: result }));
    expect(document.querySelector('[role="status"]')?.textContent).toContain("Starting Target…");
    expect(document.querySelector('[role="status"]')?.textContent).not.toBe(completedSentence);
    await act(async () => publishStatus({ ...acceptedStatus, phase: "running" }));
    expect(document.querySelector('[role="status"]')?.textContent).toContain("Switching to Target…");
    expect(document.querySelector('[role="status"]')?.hasAttribute("aria-busy")).toBe(false);
    await act(async () => publishStatus({ ...acceptedStatus, phase: "completed", outcome: result }));
    expect(document.querySelector('[role="status"]')?.textContent).toBe(completedSentence);
    expectCompletion();
  } finally { await act(async () => root.unmount()); host.remove(); }
});

it("reconciles a completed remote operation before its peer status stream is ready", async () => {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  const remoteAccepted = { ...acceptedStatus, tab_id: "remote" };
  const completed = { ...remoteAccepted, phase: "completed", outcome: result };
  const send = vi.fn(async () => remoteAccepted);
  __setTabProviderApiForTest({ switchProvider: send });
  vi.stubGlobal("fetch", (_input: RequestInfo | URL, init?: RequestInit) => new Promise<Response>((_resolve, reject) => {
    const abort = () => reject(new DOMException("aborted", "AbortError"));
    if (init?.signal?.aborted) abort();
    else init?.signal?.addEventListener("abort", abort, { once: true });
  }));
  const readStatus = mockStatusReads(() => completed);
  try {
    await act(async () => root.render(<ProviderSwitchDialog tab={remoteTab} initialProviderId="target" onClose={() => {}} />));
    await act(async () => button("Switch").click());
    expect(send).toHaveBeenCalledTimes(1);
    expect(readStatus).toHaveBeenCalledExactlyOnceWith("provider_switch_status", {
      tabId: remoteTab.id, operationId: acceptedStatus.operation_id, wait: false,
    });
    expect(document.querySelector('[role="status"]')?.textContent).toBe(completedSentence);
    expectCompletion();
  } finally { await act(async () => root.unmount()); host.remove(); }
});

it("keeps an exit action available when accepted switch status cannot be reconciled", async () => {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  const onClose = vi.fn();
  __setTabProviderApiForTest({ switchProvider: async () => acceptedStatus });
  __setInvokeForTest(async () => { throw new Error("status endpoint unavailable"); });
  try {
    await act(async () => root.render(<ProviderSwitchDialog tab={tab} initialProviderId="target" onClose={onClose} />));
    await act(async () => button("Switch").click());
    expect(document.querySelector('[role="alert"]')?.textContent).toContain("Check this tab before trying again");
    expect(document.querySelector('[role="alert"]')?.textContent).toContain("status endpoint unavailable");
    expect(button("Close").disabled).toBe(false);
    await act(async () => button("Close").click());
    expect(onClose).toHaveBeenCalledTimes(1);
  } finally { await act(async () => root.unmount()); host.remove(); }
});

it("reconciles the accepted operation on peer stream signals", async () => {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  const running = { ...acceptedStatus, tab_id: "remote", phase: "running" };
  __setTabProviderApiForTest({ switchProvider: async () => running });
  const readStatus = mockStatusReads(() => running);
  try {
    await act(async () => root.render(<ProviderSwitchDialog tab={remoteTab} initialProviderId="target" onClose={() => {}} />));
    expect(remoteSignals.handler).not.toBeNull();
    await act(async () => button("Switch").click());
    expect(readStatus).toHaveBeenCalledTimes(1);
    await act(async () => remoteSignals.handler?.("resync"));
    expect(readStatus).toHaveBeenCalledTimes(2);
    await act(async () => remoteSignals.handler?.("lost"));
    expect(readStatus).toHaveBeenCalledTimes(3);
    await act(async () => remoteSignals.handler?.("ready"));
    expect(readStatus).toHaveBeenCalledTimes(4);
    expect(document.querySelector('[role="status"]')?.textContent).toContain("Switching to Target…");
    expect(document.querySelector('[role="alert"]')).toBeNull();
  } finally { await act(async () => root.unmount()); host.remove(); }
});

it.each(["Close", "Escape"])("dismisses a running switch via %s without a stream signal", async (dismissal) => {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  const onClose = vi.fn(() => root.render(null));
  const pending = { ...acceptedStatus, tab_id: "remote" };
  const send = vi.fn(async () => pending);
  __setTabProviderApiForTest({ switchProvider: send });
  const readStatus = mockStatusReads(() => ({ ...pending, phase: "running" }));
  try {
    await act(async () => root.render(<ProviderSwitchDialog tab={remoteTab} initialProviderId="target" onClose={onClose} />));
    await act(async () => button("Switch").click());
    expect(readStatus).toHaveBeenCalledExactlyOnceWith("provider_switch_status", {
      tabId: remoteTab.id, operationId: pending.operation_id, wait: false,
    });
    expect(document.querySelector('[role="status"]')?.textContent).toContain("Switching to Target…");
    expect(document.querySelector('[role="alert"]')).toBeNull();
    const close = button("Close");
    expect(close.disabled).toBe(false);
    await act(async () => {
      if (dismissal === "Escape") {
        close.dispatchEvent(new KeyboardEvent("keydown", { code: "Escape", bubbles: true, cancelable: true }));
      } else {
        close.click();
      }
    });
    expect(onClose).toHaveBeenCalledTimes(1);
    expect(document.querySelector('[role="dialog"]')).toBeNull();
    expect(send).toHaveBeenCalledTimes(1);
    expect(readStatus).toHaveBeenCalledTimes(1);
  } finally { await act(async () => root.unmount()); host.remove(); }
});

it("drops a queued refresh when the dialog unmounts with a status read in flight", async () => {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  const running = { ...acceptedStatus, phase: "running" };
  let finishRead!: (status: ProviderSwitchStatus) => void;
  const heldRead = new Promise<ProviderSwitchStatus>((resolve) => { finishRead = resolve; });
  __setTabProviderApiForTest({ switchProvider: async () => running });
  const readStatus = mockStatusReads(() => running).mockReturnValueOnce(heldRead);
  try {
    await act(async () => root.render(<ProviderSwitchDialog tab={tab} initialProviderId="target" onClose={() => {}} />));
    await act(async () => button("Switch").click());
    expect(readStatus).toHaveBeenCalledExactlyOnceWith("provider_switch_status", {
      tabId: tab.id, operationId: running.operation_id, wait: false,
    });
    await act(async () => fireResync("overflow"));
    expect(readStatus).toHaveBeenCalledTimes(1);
    await act(async () => root.render(null));
    expect(document.querySelector('[role="dialog"]')).toBeNull();
    await act(async () => finishRead(running));
    expect(readStatus).toHaveBeenCalledTimes(1);
  } finally { await act(async () => root.unmount()); host.remove(); }
});

it("discards a switch receipt that arrives after dismissal", async () => {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  const onClose = vi.fn(() => root.render(null));
  const running = { ...acceptedStatus, phase: "running" };
  let accept!: (status: ProviderSwitchStatus) => void;
  const pending = new Promise<ProviderSwitchStatus>((resolve) => { accept = resolve; });
  const send = vi.fn(() => pending);
  __setTabProviderApiForTest({ switchProvider: send });
  const readStatus = mockStatusReads(() => running);
  try {
    await act(async () => root.render(<ProviderSwitchDialog tab={tab} initialProviderId="target" onClose={onClose} />));
    await act(async () => button("Switch").click());
    expect(send).toHaveBeenCalledTimes(1);
    await act(async () => publishStatus(running));
    expect(readStatus).not.toHaveBeenCalled();
    expect(button("Close").disabled).toBe(false);
    await act(async () => {
      button("Close").click();
      accept(acceptedStatus);
    });
    expect(onClose).toHaveBeenCalledTimes(1);
    expect(document.querySelector('[role="dialog"]')).toBeNull();
    expect(send).toHaveBeenCalledTimes(1);
    expect(readStatus).not.toHaveBeenCalled();
  } finally { await act(async () => root.unmount()); host.remove(); }
});

it("allows retry when a peer-loss refresh reports a failed switch", async () => {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  const running = { ...acceptedStatus, tab_id: "remote", phase: "running" };
  const retry = { ...running, operation_id: "retry-token" };
  const send = vi.fn().mockResolvedValueOnce(running).mockResolvedValueOnce({ ...retry, phase: "pending" });
  __setTabProviderApiForTest({ switchProvider: send });
  const readStatus = mockStatusReads(() => retry)
    .mockReturnValueOnce(running)
    .mockReturnValueOnce({ ...running, phase: "failed", error: "provider_switch_auth_timeout: fixture" });
  try {
    await act(async () => root.render(<ProviderSwitchDialog tab={remoteTab} initialProviderId="target" onClose={() => {}} />));
    expect(remoteSignals.handler).not.toBeNull();
    await act(async () => button("Switch").click());
    expect(readStatus).toHaveBeenCalledTimes(1);
    await act(async () => remoteSignals.handler?.("lost"));
    expect(readStatus).toHaveBeenCalledTimes(2);
    expect(document.querySelector('[aria-label="The provider authentication check timed out"]')).not.toBeNull();
    expect(button("Switch").disabled).toBe(false);
    await act(async () => button("Switch").click());
    expect(readStatus).toHaveBeenLastCalledWith("provider_switch_status", {
      tabId: remoteTab.id, operationId: retry.operation_id, wait: false,
    });
    expect(document.querySelector('[role="status"]')?.textContent).toContain("Switching to Target…");
    expect(send).toHaveBeenCalledTimes(2);
    expect(readStatus).toHaveBeenCalledTimes(3);
  } finally { await act(async () => root.unmount()); host.remove(); }
});

it("clears status uncertainty when a recovered snapshot repeats the same running phase", async () => {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  const running = { ...acceptedStatus, phase: "running" };
  let reads = 0;
  __setTabProviderApiForTest({ switchProvider: async () => running });
  __setInvokeForTest(async <T,>() => {
    reads += 1;
    if (reads === 1) throw new Error("temporary status read failure");
    return running as T;
  });
  try {
    await act(async () => root.render(<ProviderSwitchDialog tab={tab} initialProviderId="target" onClose={() => {}} />));
    await act(async () => button("Switch").click());
    expect(document.querySelector('[role="alert"]')?.textContent).toContain("temporary status read failure");
    expect(button("Close").disabled).toBe(false);
    await act(async () => window.dispatchEvent(new Event(EVENT_STREAM_READY)));
    expect(reads).toBe(2);
    expect(document.querySelector('[role="alert"]')).toBeNull();
    expect(document.querySelector('[role="status"]')?.textContent).toContain("Switching to Target…");
    await act(async () => publishStatus({ ...running, phase: "completed", outcome: result }));
    expect(document.querySelector('[role="status"]')?.textContent).toBe(completedSentence);
  } finally { await act(async () => root.unmount()); host.remove(); }
});

it("allows one retry after a failure event while the accepted status read is held", async () => {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  let resolveHeldRead!: (status: ProviderSwitchStatus) => void;
  let signalReadStarted!: () => void;
  const heldRead = new Promise<ProviderSwitchStatus>((resolve) => { resolveHeldRead = resolve; });
  const readStarted = new Promise<void>((resolve) => { signalReadStarted = resolve; });
  const send = vi.fn().mockResolvedValueOnce(acceptedStatus).mockResolvedValueOnce(result);
  __setTabProviderApiForTest({ switchProvider: send });
  __setInvokeForTest(async <T,>(command: string) => {
    expect(command).toBe("provider_switch_status");
    signalReadStarted();
    return await heldRead as T;
  });
  try {
    await act(async () => root.render(<ProviderSwitchDialog tab={tab} initialProviderId="target" onClose={() => {}} />));
    await act(async () => {
      button("Switch").click();
      await readStarted;
    });
    await act(async () => publishStatus({ ...acceptedStatus, phase: "failed", error: "provider_switch_auth_timeout: fixture" }));
    expect(document.querySelector('[aria-label="The provider authentication check timed out"]')).not.toBeNull();
    await act(async () => button("Switch").click());
    expect(send).toHaveBeenCalledTimes(2);
    expectCompletion();
    await act(async () => resolveHeldRead(acceptedStatus));
    expect(document.querySelector('[role="status"]')?.textContent).toBe(completedSentence);
  } finally { await act(async () => root.unmount()); host.remove(); }
});

it("recovers from a rejected initial status-listener registration on the next switch attempt", async () => {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  let registrations = 0;
  __setListenForTest(async <T,>(_event: string, handler: (value: { payload: T }) => void) => {
    registrations += 1;
    if (registrations === 1) throw new Error("listener registration refused");
    publishStatus = (status) => handler({ payload: status as T });
    return () => {};
  });
  const send = vi.fn(async () => result);
  __setTabProviderApiForTest({ switchProvider: send });
  try {
    await act(async () => root.render(<ProviderSwitchDialog tab={tab} initialProviderId="target" onClose={() => {}} />));
    await act(async () => button("Switch").click());
    expect(registrations).toBe(2);
    expect(send).toHaveBeenCalledTimes(1);
    expectCompletion();
  } finally { await act(async () => root.unmount()); host.remove(); }
});

it("reconciles the accepted operation on stream readiness, resync and loss signals", async () => {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  const send = vi.fn(async () => acceptedStatus);
  __setTabProviderApiForTest({ switchProvider: send });
  const read = mockStatusReads(() => acceptedStatus);
  try {
    await act(async () => root.render(<ProviderSwitchDialog tab={tab} initialProviderId="target" onClose={() => {}} />));
    await act(async () => button("Switch").click());
    expect(read).toHaveBeenCalledTimes(1);
    await act(async () => window.dispatchEvent(new Event(EVENT_STREAM_READY)));
    await act(async () => fireResync("overflow"));
    expect(document.querySelector('[role="alert"]')).toBeNull();
    await act(async () => window.dispatchEvent(new Event(EVENT_STREAM_LOST)));
    expect(read).toHaveBeenCalledTimes(4);
    expect(document.querySelector('[role="status"]')?.textContent).toContain("Starting Target…");
  } finally { await act(async () => root.unmount()); host.remove(); }
});

it("shows daemon failure details and keeps the existing retry path", async () => {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  const send = vi.fn().mockResolvedValueOnce(acceptedStatus).mockResolvedValueOnce(result);
  __setTabProviderApiForTest({ switchProvider: send });
  __setInvokeForTest(async <T,>() => acceptedStatus as T);
  try {
    await act(async () => root.render(<ProviderSwitchDialog tab={tab} initialProviderId="target" onClose={() => {}} />));
    await act(async () => button("Switch").click());
    await act(async () => publishStatus({ ...acceptedStatus, phase: "failed", error: "provider_switch_auth_timeout: fixture" }));
    expect(document.querySelector('[aria-label="The provider authentication check timed out"]')).not.toBeNull();
    await act(async () => button("Switch").click());
    expect(send).toHaveBeenCalledTimes(2);
    expectCompletion();
  } finally { await act(async () => root.unmount()); host.remove(); }
});


it("disables unavailable targets with a reason and refuses submission from a stale selection", async () => {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  const send = vi.fn(async () => result);
  __setTabProviderApiForTest({ switchProvider: send });
  try {
    await act(async () => root.render(<ProviderSwitchDialog tab={tab} initialProviderId="unavailable" onClose={() => {}} />));
    expect(button("Switch").disabled).toBe(true);
    const trigger = document.querySelector<HTMLButtonElement>('button[aria-label^="Agent provider:"]');
    await act(async () => trigger?.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true, cancelable: true })));
    const unavailable = Array.from(document.querySelectorAll<HTMLButtonElement>('[role="menuitemradio"]')).find(item => item.textContent?.includes("Unavailable target"));
    expect(unavailable?.disabled).toBe(true);
    expect(unavailable?.textContent).toContain("not installed");
    await act(async () => { unavailable?.click(); button("Switch").click(); });
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
    await act(async () => button("Switch").click());
    expect(document.querySelector('[aria-label="The provider authentication check timed out"]')).not.toBeNull();
    expect(send).toHaveBeenCalledTimes(1);
    await act(async () => button("Switch").click());
    expect(send).toHaveBeenCalledTimes(2);
    expectCompletion();
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
    await act(async () => button("Switch").click());
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
    await act(async () => button("Switch").click());
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
    await act(async () => button("Switch").click());
    expect(button("Switch in 19 s").disabled).toBe(true);
    await act(async () => { vi.advanceTimersByTime(18999); button("Switch in 19 s").click(); });
    expect(send).toHaveBeenCalledTimes(1);
    await act(async () => vi.advanceTimersByTime(1));
    expect(button("Switch").disabled).toBe(false);
    expect(send).toHaveBeenCalledTimes(1);
    await act(async () => button("Switch").click());
    expect(send).toHaveBeenCalledTimes(2);
  } finally { await act(async () => root.unmount()); host.remove(); }
});

it("keeps the tab restart cooldown when the target harness changes", async () => {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  const send = vi.fn().mockRejectedValueOnce(new Error("provider_switch_rate_limited: 19"));
  __setTabProviderApiForTest({ switchProvider: send });
  try {
    await act(async () => root.render(<ProviderSwitchDialog tab={tab} initialProviderId="target" onClose={() => {}} />));
    await act(async () => button("Switch").click());
    expect(button("Switch in 19 s").disabled).toBe(true);
    const picker = document.querySelector<HTMLButtonElement>('button[aria-label="Agent provider: Target"]');
    await act(async () => picker?.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true, cancelable: true })));
    const other = Array.from(document.querySelectorAll<HTMLButtonElement>('[role="menuitemradio"]'))
      .find((item) => item.textContent?.includes("Other"));
    expect(other).not.toBeUndefined();
    await act(async () => other?.click());
    expect(document.querySelector('button[aria-label="Agent provider: Other"]')).not.toBeNull();
    expect(button("Switch in 19 s").disabled).toBe(true);
    expect(send).toHaveBeenCalledTimes(1);
  } finally { await act(async () => root.unmount()); host.remove(); }
});
