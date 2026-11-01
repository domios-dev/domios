import { act, StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import { __setInvokeForTest, __setListenForTest } from "../../lib/bridge";
import type { AgentRuntimeStateEvent } from "../../lib/agentRuntimeState";
import type { ApiConfig, ProviderDescriptor, Tab, ThemeConfig } from "../../lib/types";
import {setAgentDescriptors} from "../../lib/agentDescriptorCache";
import { MuxStateStoreContext, MultiplexerActionsContext, type MultiplexerActionsContextType } from "../../store/multiplexer/context";
import { createMuxStateStore } from "../../store/multiplexer/selectorStore";
import { tabBootStageEventAction } from "../../store/multiplexer";
import { reducer } from "../../store/multiplexer/reducer";
import type { ExtendedState } from "../../store/multiplexer/tree";
import { insertIntoComposer } from "./composerInsertion";
import { RestoreStatePanel } from "./RestoreStatePanel";
import { restoreStageView } from "../../lib/restoreStageView";
import { setClientTabUiMode } from "../../lib/clientTabUiMode";
import { TabContent } from "./TabContent";
import { freeLimitGate } from "../billing/freeLimitGate";
import { getPersistedChatScrollState } from "./chatViewport";
import { readTranscriptWindow, transcriptWindowKey } from "./transcriptStore";
import type { ChatStartResult } from "./chatSessionAuthority";

vi.mock("../../hooks/useAgentProviders", () => ({ useAgentProviders: () => ({ providers: [], loading: false, error: null }) }));
vi.mock("./TerminalTab", async () => {
  const { createElement } = await import("react");
  const { TabSessionChat } = await import("./TabSessionChat");
  return {
    TerminalTab: ({ tabId, api, isVisible }: { tabId: string; api?: ApiConfig; isVisible?: boolean }) =>
      createElement(TabSessionChat, { tabId, api, isVisible }),
  };
});
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

it("makes the previous kept-alive composer inert and releases focus in the tab-switch commit", async () => {
  const tabs: Tab[] = ["input-a", "input-b"].map(id => ({
    id, title: id, tab_type: "Terminal", cwd: null, pty_id: null,
    backing: { kind: "chat_backend", plugin_id: "fixture", session_id: id },
    session_binding_nonce: id, agent_runtime_state: { state: "loaded", epoch: 1 },
  }));
  const state: ExtendedState = { workspaces: [], tabs: Object.fromEntries(tabs.map(tab => [tab.id, tab])), teams: [],
    active_workspace_id: null, focused_tab_id: tabs[0].id, notifications: [], tabStatuses: {}, toasts: [],
    activeTabIds: {}, generation: 1, pendingCloses: [] };
  __setListenForTest(async () => () => {});
  __setInvokeForTest(async <T,>(command: string, args?: Record<string, unknown>) => {
    const value = command === "chat_start" ? { messages: [], pending: false, text: "", offset: 0,
      session_binding_nonce: args?.tabId }
      : command === "delayed_send_list" || command === "list_agent_providers" ? []
      : command === "plugin_manager_snapshot" ? { plugins: [], channels: [], diff: [], marketplace: [] } : {};
    return value as T;
  });
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  const store = createMuxStateStore(() => state);
  const actions = new Proxy({}, { get: () => () => {} }) as MultiplexerActionsContextType;
  const render = (active: string) => act(async () => root.render(
    <MuxStateStoreContext.Provider value={store}><MultiplexerActionsContext.Provider value={actions}>
      {tabs.map(tab => <TabContent key={tab.id} tab={tab} isVisible={tab.id === active} isFocused={tab.id === active}
        onFocusTab={() => {}} onPtySpawned={() => {}}
        theme={{ name: "fixture", background: "#000", foreground: "#fff" } as ThemeConfig} />)}
    </MultiplexerActionsContext.Provider></MuxStateStoreContext.Provider>,
  ));
  try {
    await render(tabs[0].id);
    const a = host.querySelector<HTMLTextAreaElement>('[data-tab-id="input-a"] textarea')!;
    const b = host.querySelector<HTMLTextAreaElement>('[data-tab-id="input-b"] textarea')!;
    const keyTargets: string[] = [];
    a.addEventListener("keydown", () => keyTargets.push("a"));
    b.addEventListener("keydown", () => keyTargets.push("b"));
    a.focus();
    expect(document.activeElement).toBe(a);
    await render(tabs[1].id);
    expect(host.querySelector('[data-tab-id="input-a"] textarea')).toBe(a);
    expect(a.closest("[inert][aria-hidden=true]")).not.toBeNull();
    expect(document.activeElement).not.toBe(a);
    expect(b.closest("[inert]")).toBeNull();
    b.click(); b.focus();
    await act(async () => {
      document.activeElement?.dispatchEvent(new KeyboardEvent("keydown", { key: "x", code: "KeyX", bubbles: true }));
      Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(b, "only-b");
      b.dispatchEvent(new Event("input", { bubbles: true }));
    });
    expect(document.activeElement).toBe(b);
    expect(keyTargets).toEqual(["b"]);
    expect(b.value).toBe("only-b");
    expect(a.value).toBe("");
    await render(tabs[0].id);
    expect(b.closest("[inert][aria-hidden=true]")).not.toBeNull();
    expect(document.activeElement).not.toBe(b);
    expect(a.closest("[inert]")).toBeNull();
  } finally {
    await act(async () => root.unmount()); host.remove();
    __setInvokeForTest(null); __setListenForTest(null);
  }
});

it("shows the Team lock surface instead of attempting to wake its unlocked sleeping tab", async () => {
  const tab: Tab = { id: "team-locked-wake", title: "Fixture", tab_type: "Terminal", cwd: null, pty_id: null,
    team_id: "locked-team", frozen: true, agent_type: "fixture", slept_at_ms: 1000,
    backing: { kind: "chat_backend", plugin_id: "fixture", session_id: "saved" } };
  const state: ExtendedState = { workspaces: [], tabs: { [tab.id]: tab },
    teams: [{ id: "locked-team", lifecycle: "active", created_at: 0, workspace_id: "workspace", locked: true }],
    active_workspace_id: null, focused_tab_id: tab.id, notifications: [], tabStatuses: {}, toasts: [],
    activeTabIds: {}, generation: 1, pendingCloses: [] };
  const calls: string[] = [];
  __setListenForTest(async () => () => {});
  __setInvokeForTest(async <T,>(command: string) => { calls.push(command); return {} as T; });
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  freeLimitGate.close();
  const actions = new Proxy({}, { get: () => () => {} }) as MultiplexerActionsContextType;
  try {
    await act(async () => root.render(<MuxStateStoreContext.Provider value={createMuxStateStore(() => state)}>
      <MultiplexerActionsContext.Provider value={actions}>
      <TabContent tab={tab} isFocused isVisible onFocusTab={() => {}} onPtySpawned={() => {}}
        theme={{ name: "fixture", background: "#000", foreground: "#fff" } as ThemeConfig} />
      </MultiplexerActionsContext.Provider>
    </MuxStateStoreContext.Provider>));
    expect(host.querySelector('[aria-label="Locked: free plan limit"]')).not.toBeNull();
    expect(host.querySelector('[role="alert"]')).toBeNull();
    expect(calls).not.toContain("wake_tab");
    expect(calls).not.toContain("chat_start");
    const unlock = host.querySelector<HTMLButtonElement>('[data-testid="lock-overlay"]')!;
    expect(unlock).not.toBeNull();
    await act(async () => unlock.click());
    expect(freeLimitGate.prompt()).toBe("teams");
  } finally {
    await act(async () => root.unmount()); host.remove(); freeLimitGate.close();
    __setInvokeForTest(null); __setListenForTest(null);
  }
});

it("routes a non-PTY task tab through its backing provider even without a local agent identity", async () => {
  const tab: Tab = {id: "remote-layout", title: "Fixture", tab_type: "Terminal", cwd: null, pty_id: null,
    agent_runtime_config: {executionMode: "cloud"}, backing: {kind: "chat_backend", plugin_id: "remote-fixture", session_id: "remote-session"}};
  const descriptor: ProviderDescriptor = {
    schema_version: 1, id: "remote-fixture", display_name: "Remote fixture", transport: "chat_backend", system_prompt_delivery: "external",
    available_models: [], settings_schema: [], notifications_schema: [], supports_lazy_skills: false, has_hooks: false,
    remote_task: {backend: "fixture", mode: "cloud", displayName: "Cloud", availability: {status: "available"}, environments: [], followUp: false, cancel: false, applyDiff: false},
  };
  const state: ExtendedState = {workspaces: [], tabs: {[tab.id]: tab}, teams: [], active_workspace_id: null, focused_tab_id: tab.id,
    notifications: [], tabStatuses: {}, toasts: [], activeTabIds: {}, generation: 1, pendingCloses: []};
  const calls: string[] = [];
  const onPtySpawned = vi.fn();
  __setListenForTest(async () => () => {});
  __setInvokeForTest(async <T,>(command: string) => {
    calls.push(command);
    const value = command === "plugin_remote_task_call" ? {phase: "bound", snapshot: {status: "ready", result: "Fixture remote result", diff: null}}
      : command === "plugin_manager_snapshot" ? {plugins: [], channels: [], diff: [], marketplace: []} : {};
    return value as T;
  });
  setAgentDescriptors([descriptor], false);
  const host = document.createElement("div"); const root = createRoot(host);
  const actions = new Proxy({}, {get: () => () => {}}) as MultiplexerActionsContextType;
  try {
    await act(async () => root.render(<MuxStateStoreContext.Provider value={createMuxStateStore(() => state)}>
      <MultiplexerActionsContext.Provider value={actions}>
        <TabContent tab={tab} isFocused isVisible onFocusTab={() => {}} onPtySpawned={onPtySpawned}
          theme={{name: "fixture", background: "#000", foreground: "#fff"} as ThemeConfig} />
      </MultiplexerActionsContext.Provider>
    </MuxStateStoreContext.Provider>));
    expect(host.querySelector('section[aria-label="Remote task"]')).not.toBeNull();
    expect(host.querySelector("pre")?.textContent).toBe("Fixture remote result");
    expect(calls).toContain("plugin_remote_task_call");
    expect(calls).not.toContain("chat_start");
    expect(onPtySpawned).not.toHaveBeenCalled();
  } finally {
    await act(async () => root.unmount()); setAgentDescriptors([], false);
    __setInvokeForTest(null); __setListenForTest(null);
  }
});

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

it("keeps chat startup visible through a pending runtime response deadline", async () => {
  const tab: Tab = {
    id: "pending-deadline", title: "Fixture", tab_type: "Terminal", cwd: null, pty_id: null,
    backing: { kind: "chat_backend", plugin_id: "fixture", session_id: "thread" },
    agent_type: "fixture", agent_session_id: "thread", session_binding_nonce: "binding",
    boot_stage: "agent_launching", agent_booting: true,
    agent_runtime_state: { state: "degraded", epoch: 2, reason: "Deadline: opaque" },
  };
  const state: ExtendedState = {
    workspaces: [], tabs: { [tab.id]: tab }, teams: [], active_workspace_id: null, focused_tab_id: tab.id,
    notifications: [], tabStatuses: {}, toasts: [], activeTabIds: {}, generation: 1, pendingCloses: [],
  };
  __setListenForTest(async () => () => {});
  __setInvokeForTest(async <T,>(command: string) => {
    if (command === "chat_start") return new Promise<T>(() => {});
    const value = command === "delayed_send_list" || command === "list_agent_providers" ? []
      : command === "plugin_manager_snapshot" ? { plugins: [], channels: [], diff: [], marketplace: [] }
      : {};
    return value as T;
  });
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const actions = new Proxy({}, { get: () => () => {} }) as MultiplexerActionsContextType;
  try {
    await act(async () => root.render(
      <MuxStateStoreContext.Provider value={createMuxStateStore(() => state)}>
        <MultiplexerActionsContext.Provider value={actions}>
          <TabContent tab={tab} isFocused isVisible onFocusTab={() => {}} onPtySpawned={() => {}}
            theme={{ name: "fixture", background: "#000", foreground: "#fff" } as ThemeConfig} />
        </MultiplexerActionsContext.Provider>
      </MuxStateStoreContext.Provider>,
    ));
    expect(host.querySelector('[data-testid="chat-setup-state"]')).not.toBeNull();
    expect(host.querySelector('[role="progressbar"][aria-valuetext]')).not.toBeNull();
    expect(host.querySelector('[role="status"]')).not.toBeNull();
    expect(host.querySelector('[data-testid="chat-launch-failed"]')).toBeNull();
    expect(host.querySelector('[role="alert"]')).toBeNull();
    const recoveryButtons = [...host.querySelectorAll("button")].filter(button =>
      /retry|wake again/i.test((button.getAttribute("aria-label") ?? "") + " " + (button.textContent ?? "")));
    expect(recoveryButtons).toHaveLength(0);
  } finally {
    await act(async () => root.unmount());
    host.remove();
    __setInvokeForTest(null);
    __setListenForTest(null);
  }
});


it("requests one wake when a sleeping tab becomes visible without awaiting materialization", async () => {
  const tab: Tab = {
    id: "wake-visible-fixture", title: "Fixture", tab_type: "Terminal", cwd: null, pty_id: null, frozen: true, agent_type: "fixture",
    slept_at_ms: 1000, agent_session_id: "saved", session_binding_nonce: "sleep-binding",
    backing: { kind: "chat_backend", plugin_id: "fixture", session_id: "saved" },
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
    if (command === "chat_start") return Promise.resolve({
      text: "", pending: false, start_offset: 0, offset: 1,
      source_session_id: "saved", session_binding_nonce: "sleep-binding", subscription_id: args?.subscriptionId,
      messages: [{ id: "saved-row", type: "assistant", content: "persisted-fixture-content" }],
    } as T);
    return Promise.resolve((command === "delayed_send_list" ? [] : {}) as T);
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
    expect(host.querySelector('[role="status"][aria-label="Sleeping"]')).toBeNull();
    await render(true);
    expect(calls).toEqual([tab.id]);
    expect(host.textContent).toContain("persisted-fixture-content");
    expect([...host.querySelectorAll('[role="status"]')].some(status => status.getAttribute("aria-label") === "Restoring…")).toBe(true);
    expect(host.querySelector('[role="progressbar"][aria-label="Restore progress"]')).toBeNull();
    const composer = host.querySelector("textarea");
    expect(composer).not.toBeNull();
    await render(false);
    await render(true);
    expect(calls).toEqual([tab.id]);
    tab.frozen = false;
    tab.slept_at_ms = null;
    tab.boot_stage = "agent_ready";
    await render(true);
    expect(host.querySelector("textarea")).toBe(composer);
    expect(host.textContent).toContain("persisted-fixture-content");
  } finally {
    await act(async () => root.unmount());
    host.remove();
    __setInvokeForTest(null);
    __setListenForTest(null);
  }
});

it("releases a restored PTY Chat on its own agent_ready lifecycle without PTY output", async () => {
  const tab: Tab = {
    id: "restored-pty-chat", title: "Grok", tab_type: "Terminal", cwd: null, pty_id: "pty-own",
    backing: { kind: "pty" }, agent_type: "fixture", agent_session_id: "own-session",
    session_binding_nonce: "old-binding", frozen: true, slept_at_ms: null, boot_stage: "pty_starting",
    lifecycle_epoch: 1, lifecycle_operation_id: "prior-launch",
  };
  const otherTab: Tab = {
    ...tab, id: "other-pty-chat", pty_id: "pty-other", agent_session_id: "other-session",
    session_binding_nonce: "other-binding", boot_stage: "pty_starting",
  };
  let state: ExtendedState = {
    workspaces: [], tabs: { [tab.id]: tab, [otherTab.id]: otherTab }, teams: [], active_workspace_id: null, focused_tab_id: tab.id,
    notifications: [], tabStatuses: {}, toasts: [], activeTabIds: {}, generation: 1, pendingCloses: [],
  };
  const store = createMuxStateStore(() => state);
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const actions = new Proxy({}, { get: () => () => {} }) as MultiplexerActionsContextType;
  __setListenForTest(async () => () => {});
  __setInvokeForTest(async <T,>(command: string) => {
    const value = command === "delayed_send_list" || command === "list_agent_providers" ? []
      : command === "plugin_manager_snapshot" ? { plugins: [], channels: [], diff: [], marketplace: [] }
      : command === "chat_start" ? { messages: [], pending: false, text: "", start_offset: 0, offset: 0,
        source_session_id: "own-session", session_binding_nonce: state.tabs[tab.id].session_binding_nonce }
      : command === "wake_tab" ? { accepted: true }
      : {};
    return value as T;
  });
  setClientTabUiMode(tab.id, "chat");
  const render = async () => act(async () => root.render(
    <MuxStateStoreContext.Provider value={store}>
      <MultiplexerActionsContext.Provider value={actions}>
        <TabContent tab={state.tabs[tab.id]} isFocused isVisible onFocusTab={() => {}} onPtySpawned={() => {}}
          theme={{ name: "fixture", background: "#000", foreground: "#fff" } as ThemeConfig} />
      </MultiplexerActionsContext.Provider>
    </MuxStateStoreContext.Provider>,
  ));
  try {
    await render();
    expect(host.querySelector('[role="status"][aria-label="Restoring…"]')).not.toBeNull();
    expect(host.querySelector('[aria-label="Message composer"]')).toBeNull();

    state = reducer(state, tabBootStageEventAction({
      tab_id: tab.id,
      stage: "agent_ready",
      agent_booting: false,
      session_bound: true,
      launch_generation: { epoch: 2, operation_id: "restored-launch", pty_id: "pty-restored", binding_nonce: "restored-binding" },
    }));
    await act(async () => store.notify());
    await render();

    expect(state.tabs[otherTab.id].frozen).toBe(true);
    expect(state.tabs[tab.id].pty_id).toBe("pty-restored");
    expect(state.tabs[tab.id].session_binding_nonce).toBe("restored-binding");
    expect(host.querySelector('[role="status"][aria-label="Restoring…"]')).toBeNull();
    const composer = host.querySelector<HTMLTextAreaElement>('textarea[aria-label="Message composer"]');
    expect(composer).not.toBeNull();
    expect(composer?.disabled).toBe(false);
    expect(composer?.closest("[inert]")).toBeNull();
  } finally {
    await act(async () => root.unmount());
    host.remove();
    setClientTabUiMode(tab.id, "terminal");
    __setInvokeForTest(null);
    __setListenForTest(null);
  }
});

it("projects sleeping, restoring and failed snapshots without a wake control", async () => {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const render = (tab: Partial<Tab>) => act(async () => root.render(
    <RestoreStatePanel view={restoreStageView(tab)} />,
  ));
  try {
    await render({ frozen: true });
    expect(host.querySelector('[role="status"][aria-label="Sleeping"]')).not.toBeNull();
    expect(host.querySelector(".brand-mark")).not.toBeNull();
    await render({ frozen: false, boot_stage: "pty_starting" });
    expect(host.querySelector('[role="status"][aria-label="Restoring…"]')).not.toBeNull();
    await render({ frozen: false, boot_stage: "spawn_failed", boot_failure_reason: "fixture-error" });
    expect(host.querySelector('[role="status"]')?.textContent).toContain("fixture-error");
    expect([...host.querySelectorAll("button")].filter(button => /wake/i.test(button.textContent ?? ""))).toHaveLength(0);
  } finally {
    await act(async () => root.unmount());
    host.remove();
    __setInvokeForTest(null);
    __setListenForTest(null);
  }
});

it("refreshes only its chat attachment while retaining draft, authority, transcript and detached scroll", async () => {
  let tab: Tab = {
    id: "header-refresh-own", title: "Fixture", tab_type: "Terminal", cwd: null, pty_id: null,
    backing: { kind: "chat_backend", plugin_id: "fixture", session_id: "refresh-session" },
    agent_session_id: "refresh-session", session_binding_nonce: "refresh-binding", boot_stage: "agent_ready",
    agent_runtime_state: { state: "loaded", epoch: 1 },
  };
  const other: Tab = { ...tab, id: "header-refresh-other", agent_session_id: "other-session",
    session_binding_nonce: "other-binding", backing: { kind: "chat_backend", plugin_id: "fixture", session_id: "other-session" } };
  let state: ExtendedState = { workspaces: [], tabs: { [tab.id]: tab, [other.id]: other }, teams: [],
    active_workspace_id: null, focused_tab_id: tab.id, notifications: [], tabStatuses: {}, toasts: [],
    activeTabIds: {}, generation: 1, pendingCloses: [] };
  const store = createMuxStateStore(() => state);
  const calls: Array<{ command: string; args?: Record<string, unknown> }> = [];
  const subscriptions = new Set<string>();
  let completeRefresh: ((result: ChatStartResult) => void) | undefined;
  let refreshedSubscription = "";
  const snapshot = (sessionId: string, nonce: string, subscriptionId: string): ChatStartResult => ({
    text: "", offset: 1, start_offset: 0, pending: false, source_session_id: sessionId,
    session_binding_nonce: nonce, subscription_id: subscriptionId,
    messages: [{ id: `${sessionId}-row`, type: "assistant", content: `${sessionId} retained reply` }],
  });
  __setListenForTest(async () => () => {});
  __setInvokeForTest(<T,>(command: string, args?: Record<string, unknown>): Promise<T> => {
    calls.push({ command, args });
    if (command === "chat_start") {
      const own = args?.tabId === tab.id;
      const subscriptionId = String(args?.subscriptionId);
      if (own && calls.filter(call => call.command === "chat_start" && call.args?.tabId === tab.id).length === 2) {
        refreshedSubscription = subscriptionId;
        return new Promise<ChatStartResult>(resolve => { completeRefresh = resolve; }) as Promise<T>;
      }
      subscriptions.add(subscriptionId);
      return Promise.resolve(snapshot(own ? "refresh-session" : "other-session", own ? "refresh-binding" : "other-binding", subscriptionId) as T);
    }
    if (command === "chat_stop") subscriptions.delete(String(args?.subscriptionId));
    const value = command === "delayed_send_list" || command === "list_agent_providers" ? []
      : command === "plugin_manager_snapshot" ? { plugins: [], channels: [], diff: [], marketplace: [] } : {};
    return Promise.resolve(value as T);
  });
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  const actions = new Proxy({}, { get: () => () => {} }) as MultiplexerActionsContextType;
  const render = () => act(async () => root.render(
    <MuxStateStoreContext.Provider value={store}><MultiplexerActionsContext.Provider value={actions}>
      {[tab, other].map(item => <TabContent key={item.id} tab={item} isFocused={item === tab} isVisible
        onFocusTab={() => {}} onPtySpawned={() => {}}
        theme={{ name: "fixture", background: "#000", foreground: "#fff" } as ThemeConfig} />)}
    </MultiplexerActionsContext.Provider></MuxStateStoreContext.Provider>,
  ));
  const ownSurface = () => host.querySelector(`[data-tab-id="${tab.id}"]`)!;
  try {
    await render();
    const composer = ownSurface().querySelector<HTMLTextAreaElement>("textarea")!;
    const row = ownSurface().querySelector('[data-testid="chat-message"]');
    const scroll = ownSurface().querySelector<HTMLDivElement>('[data-testid="tab-session-chat-messages"]')!;
    expect(row?.textContent).toContain("refresh-session retained reply");
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(composer, "unsent refresh draft");
      composer.dispatchEvent(new Event("input", { bubbles: true }));
      Object.defineProperties(scroll, { scrollHeight: { configurable: true, value: 1000 }, clientHeight: { configurable: true, value: 200 } });
      scroll.scrollTop = 800;
      scroll.dispatchEvent(new Event("scroll", { bubbles: true }));
    });
    await act(async () => {
      scroll.scrollTop = 200;
      scroll.dispatchEvent(new Event("scroll", { bubbles: true }));
    });
    const scrollKey = `${tab.id}:refresh-session`;
    expect(getPersistedChatScrollState(scrollKey)?.mode).toBe("detached");
    const authority = readTranscriptWindow(transcriptWindowKey(tab.id, "refresh-session")).transcript.authority;
    expect(authority).toEqual({ sourceSessionId: "refresh-session", sessionBindingNonce: "refresh-binding" });
    const refresh = ownSurface().querySelector<HTMLButtonElement>('button[aria-label="Refresh chat"]')!;
    expect(refresh).not.toBeNull();
    await act(async () => refresh.click());
    expect(completeRefresh).toBeDefined();
    expect(ownSurface().querySelector("textarea")).toBe(composer);
    expect(composer.value).toBe("unsent refresh draft");
    expect(ownSurface().querySelector('[data-testid="chat-message"]')).toBe(row);
    expect(scroll.scrollTop).toBe(200);
    expect(subscriptions.size).toBe(1);
    await act(async () => {
      subscriptions.add(refreshedSubscription);
      completeRefresh!(snapshot("refresh-session", "refresh-binding", refreshedSubscription));
    });
    expect(subscriptions.size).toBe(2);
    expect(calls.filter(call => call.command === "chat_start").map(call => [call.args?.tabId, call.args?.agentSessionId])).toEqual([
      [tab.id, "refresh-session"], [other.id, "other-session"], [tab.id, "refresh-session"],
    ]);
    expect(readTranscriptWindow(transcriptWindowKey(tab.id, "refresh-session")).transcript.authority).toEqual(authority);
    expect(composer.value).toBe("unsent refresh draft");
    expect(ownSurface().querySelector('[data-testid="chat-message"]')).toBe(row);
    expect(ownSurface().querySelector('[data-testid="tab-session-chat-messages"]')).toBe(scroll);
    expect(getPersistedChatScrollState(scrollKey)?.mode).toBe("detached");
    expect(scroll.scrollTop).toBe(200);
    expect(calls.some(call => ["chat_send_text", "chat_send_raw", "send_text", "restart_agent_tab", "agent_runtime_retry"].includes(call.command))).toBe(false);

    tab = { ...tab, agent_runtime_state: { state: "degraded", epoch: 2, reason: "Deadline: opaque" } };
    state = { ...state, tabs: { ...state.tabs, [tab.id]: tab } };
    await act(async () => store.notify());
    await render();
    const restoring = ownSurface().querySelector('[role="status"][aria-label="Restoring…"]');
    expect(restoring === null || restoring.closest('[aria-hidden="true"]') !== null).toBe(true);
    expect(ownSurface().querySelector('[aria-label="Agent runtime is taking longer than usual"]')).not.toBeNull();
    expect(ownSurface().querySelector("textarea")).toBe(composer);
    tab = { ...tab, agent_runtime_state: { state: "degraded", epoch: 3, reason: "ProcessExited: opaque" } };
    state = { ...state, tabs: { ...state.tabs, [tab.id]: tab } };
    await act(async () => store.notify());
    await render();
    expect(ownSurface().querySelector('[aria-label="Agent runtime unavailable"]')).not.toBeNull();
  } finally {
    await act(async () => root.unmount()); host.remove();
    __setInvokeForTest(null); __setListenForTest(null);
  }
  expect(subscriptions.size).toBe(0);
});
