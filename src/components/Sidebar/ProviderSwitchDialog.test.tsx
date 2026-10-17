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
  { id: "target", display_name: "Target", transport: "chat_backend", agent_runtime: {}, available_models: [{ id: "model-token", display_name: "Model", reasoning_efforts: [{ id: "effort-token", display_name: "Effort" }] }] },
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
    expect(document.querySelector('[aria-label="Target provider"]')).not.toBeNull();
    expect(document.querySelector('[aria-label="Target model"]')).not.toBeNull();
    expect(document.querySelector('[aria-label="Reasoning effort"]')).not.toBeNull();
    expect(send).not.toHaveBeenCalled();
    await act(async () => { const confirm = button("Switch provider"); confirm.click(); confirm.click(); });
    expect(send).toHaveBeenCalledExactlyOnceWith("owned", "target", { model: "model-token", reasoningEffort: "effort-token", force: false });
    await act(async () => finish(result));
    expect(button("Previous provider: source")).not.toBeNull();
    __setInvokeForTest(async <T,>(command: string) => {
      expect(command).toBe("read_provider_history");
      return [{ kind: "user", content: "opaque-prior-🪵" }] as T;
    });
    await act(async () => button("Previous provider: source").click());
    expect(document.querySelector('[aria-label="Previous provider transcript"]')?.textContent).toContain("opaque-prior-🪵");
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
    expect(document.querySelector('[role="alert"]')?.textContent).toContain("opaque-refusal");
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
