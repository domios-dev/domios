import { act, StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import { __setInvokeForTest, __setListenForTest } from "../../lib/bridge";
import type { AgentRuntimeStateEvent } from "../../lib/agentRuntimeState";
import type { Tab, ThemeConfig } from "../../lib/types";
import { MuxStateStoreContext, MultiplexerActionsContext, type MultiplexerActionsContextType } from "../../store/multiplexer/context";
import { createMuxStateStore } from "../../store/multiplexer/selectorStore";
import type { ExtendedState } from "../../store/multiplexer/tree";
import { insertIntoComposer } from "./composerInsertion";
import { RestoreStatePanel } from "./RestoreStatePanel";
import { restoreStageView } from "../../lib/restoreStageView";
import { TabContent } from "./TabContent";

vi.mock("../../hooks/useAgentProviders", () => ({ useAgentProviders: () => ({ providers: [], loading: false, error: null }) }));
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

it("keeps the composer draft and attachment through runtime status changes", async () => {
  const tab: Tab = {
    id: "runtime-layout", title: "Fixture", tab_type: "Terminal", cwd: null, pty_id: null,
    backing: { kind: "chat_backend", plugin_id: "fixture", session_id: "thread" },
    session_binding_nonce: "binding", agent_runtime_state: { state: "loaded", epoch: 1 },
  };
  const state: ExtendedState = {
    workspaces: [], tabs: { [tab.id]: tab }, teams: [], active_workspace_id: null, focused_tab_id: tab.id,
    notifications: [], tabStatuses: {}, toasts: [], activeTabIds: {}, generation: 1, pendingCloses: [],
  };
  const handlers = new Map<string, (event: { payload: unknown }) => void>();
  __setListenForTest(async (event, handler) => {
    handlers.set(event, handler as (event: { payload: unknown }) => void);
    return () => { handlers.delete(event); };
  });
  __setInvokeForTest(async <T,>(command: string) => {
    const value = command === "delayed_send_list" || command === "list_agent_providers" ? []
      : command === "plugin_manager_snapshot" ? { plugins: [], channels: [], diff: [], marketplace: [] }
      : command === "chat_start" ? { messages: [], pending: false, text: "", offset: 0, source_session_id: null, session_binding_nonce: "binding" }
      : {};
    return value as T;
  });
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const actions = new Proxy({}, { get: () => () => {} }) as MultiplexerActionsContextType;
  const emit = async (runtimeState: AgentRuntimeStateEvent["state"]) => {
    await act(async () => handlers.get("agent-runtime-state")?.({ payload: {
      tab_id: tab.id, session_binding_nonce: "binding", state: runtimeState,
    } satisfies AgentRuntimeStateEvent }));
  };
  try {
    await act(async () => root.render(
      <MuxStateStoreContext.Provider value={createMuxStateStore(() => state)}>
        <MultiplexerActionsContext.Provider value={actions}>
          <TabContent tab={tab} isFocused isVisible onFocusTab={() => {}} onPtySpawned={() => {}}
            theme={{ name: "fixture", background: "#000", foreground: "#fff" } as ThemeConfig} />
        </MultiplexerActionsContext.Provider>
      </MuxStateStoreContext.Provider>,
    ));
    const header = host.querySelector('[data-testid="tab-top-bar"]');
    expect(header).not.toBeNull();
    const input = host.querySelector<HTMLTextAreaElement>("textarea")!;
    expect(input).not.toBeNull();
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(input, "unsent fixture ");
      input.dispatchEvent(new Event("input", { bubbles: true }));
    });
    await act(async () => insertIntoComposer(tab.id, [{ kind: "document", path: "/tmp/preserved.md" }]));
    const draft = input.value;
    expect(draft).toContain("unsent fixture");
    expect(draft).not.toBe("unsent fixture ");
    const attachment = host.querySelector('[data-testid="composer-capsule-mirror"]')?.textContent;
    expect(attachment).toContain("preserved.md");
    await emit({ state: "degraded", epoch: 1, reason: "fixture runtime fault" });
    const badge = host.querySelector<HTMLButtonElement>('button[aria-haspopup="dialog"][aria-label="Agent runtime unavailable"]');
    expect(badge).not.toBeNull();
    expect(host.querySelector<HTMLTextAreaElement>("textarea")?.value).toBe(draft);
    for (const runtimeState of ["loading", "unloaded"] as const) {
      await emit({ state: runtimeState, epoch: 2, reason: runtimeState === "unloaded" ? "pipe ended" : undefined });
      expect(host.querySelector<HTMLTextAreaElement>("textarea")?.value).toBe(draft);
    }
    await emit({ state: "loaded", epoch: 2 });
    expect(host.querySelector<HTMLTextAreaElement>("textarea")?.value).toBe(draft);
    expect(host.querySelector('[data-testid="composer-capsule-mirror"]')?.textContent).toBe(attachment);
    expect(host.querySelector('button[aria-label="Retry"]')).toBeNull();
  } finally {
    await act(async () => root.unmount());
    host.remove();
    __setInvokeForTest(null);
    __setListenForTest(null);
  }
});


it("requests one wake when a sleeping tab becomes visible without awaiting materialization", async () => {
  const tab: Tab = {
    id: "wake-visible-fixture", title: "Fixture", tab_type: "Terminal", cwd: null, pty_id: null, frozen: true,
  };
  const state: ExtendedState = {
    workspaces: [], tabs: { [tab.id]: tab }, teams: [], active_workspace_id: null, focused_tab_id: tab.id,
    notifications: [], tabStatuses: {}, toasts: [], activeTabIds: {}, generation: 1, pendingCloses: [],
  };
  const calls: string[] = [];
  __setInvokeForTest(<T,>(command: string, args?: Record<string, unknown>): Promise<T> => {
    if (command === "wake_tab") {
      calls.push(String(args?.tabId));
      return new Promise<T>(() => {});
    }
    return Promise.resolve({} as T);
  });
  __setListenForTest(async () => () => {});
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const store = createMuxStateStore(() => state);
  const actions = new Proxy({}, { get: () => () => {} }) as MultiplexerActionsContextType;
  const render = (visible: boolean) => act(async () => root.render(
    <StrictMode><MuxStateStoreContext.Provider value={store}>
      <MultiplexerActionsContext.Provider value={actions}>
        <TabContent tab={tab} isFocused={visible} isVisible={visible} onFocusTab={() => {}} onPtySpawned={() => {}}
          theme={{ name: "fixture", background: "#000", foreground: "#fff" } as ThemeConfig} />
      </MultiplexerActionsContext.Provider>
    </MuxStateStoreContext.Provider></StrictMode>,
  ));
  try {
    await render(false);
    expect(calls).toEqual([]);
    expect(host.querySelector('[role="status"][aria-label="Sleeping"]')).not.toBeNull();
    expect(host.querySelector('[role="img"][aria-label="Session provider icon"]')).not.toBeNull();
    await render(true);
    expect(calls).toEqual([tab.id]);
    expect(host.querySelector('[role="status"][aria-label="Restoring…"]')).not.toBeNull();
    await render(false);
    await render(true);
    expect(calls).toEqual([tab.id]);
  } finally {
    await act(async () => root.unmount());
    host.remove();
    __setInvokeForTest(null);
    __setListenForTest(null);
  }
});

it("projects sleeping, restoring and failed snapshots into the state panel and wires one retry", async () => {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const retry = vi.fn();
  const render = (tab: Partial<Tab>) => act(async () => root.render(
    <RestoreStatePanel providerId="fixture" view={restoreStageView(tab)} onRetry={retry} />,
  ));
  try {
    await render({ frozen: true });
    expect(host.querySelector('[role="status"][aria-label="Sleeping"]')).not.toBeNull();
    expect(host.querySelector('[role="img"][aria-label="Session provider icon"]')).not.toBeNull();
    await render({ frozen: false, boot_stage: "pty_starting" });
    expect(host.querySelector('[role="status"][aria-label="Restoring…"]')).not.toBeNull();
    await render({ frozen: false, boot_stage: "spawn_failed", boot_failure_reason: "fixture-error" });
    expect(host.querySelector('[role="status"]')?.textContent).toContain("fixture-error");
    const button = [...host.querySelectorAll("button")].find(button => button.textContent === "Wake again")!;
    await act(async () => button.click());
    expect(retry).toHaveBeenCalledTimes(1);
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});
