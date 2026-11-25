import { act, StrictMode, type ComponentProps } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import { __setInvokeForTest, __setListenForTest } from "../../lib/bridge";
import { PeerTabRelayError } from "../../lib/remoteChatState";
import type { AgentRuntimeStateEvent } from "../../lib/agentRuntimeState";
import type { ProviderDescriptor, Tab, ThemeConfig } from "../../lib/types";
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
import { reconnectTabSession } from "./tabSessionActions";

vi.mock("../../hooks/useAgentProviders", () => ({ useAgentProviders: () => ({ providers: [], loading: false, error: null }) }));
vi.mock("./TerminalTab", async () => {
  const { createElement } = await import("react");
  const { TabSessionChat } = await import("./TabSessionChat");
  return {
    TerminalTab: ({ tabId, api, isVisible, frozenRestore }: Pick<ComponentProps<typeof TabSessionChat>, "tabId" | "api" | "isVisible" | "frozenRestore">) =>
      createElement(TabSessionChat, { tabId, api, isVisible, frozenRestore }),
  };
});
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

it("shows sign-in inside General while the bound runtime is signed out and clears it on sign-in", async () => {
  const tab: Tab = {
    id: "general-auth", title: "Main", tab_type: "Terminal", cwd: null, pty_id: null, agent_type: "fixture",
    role_profile: "general_agent", protected: true, agent_session_id: "general-session", session_binding_nonce: "general-binding",
    backing: { kind: "chat_backend", plugin_id: "fixture", session_id: "general-session" },
    agent_runtime_state: { state: "loginPending", epoch: 1, auth: "signedOut", loginProviderName: "Fixture", loginMethods: ["browser"] },
  };
  const state: ExtendedState = { workspaces: [], tabs: { [tab.id]: tab }, teams: [], active_workspace_id: null, focused_tab_id: tab.id,
    notifications: [], tabStatuses: {}, toasts: [], activeTabIds: {}, generation: 1, pendingCloses: [] };
  let publish!: (event: { payload: AgentRuntimeStateEvent }) => void;
  __setListenForTest(async (name, handler) => {
    if (name === "agent-runtime-state") publish = handler as typeof publish;
    return () => {};
  });
  __setInvokeForTest(<T,>(command: string, args?: Record<string, unknown>): Promise<T> => {
    if (command === "chat_start") return Promise.resolve({
      text: "", offset: 0, start_offset: 0, pending: false, messages: [], source_session_id: "general-session",
      session_binding_nonce: "general-binding", subscription_id: args?.subscriptionId,
    } as T);
    const value = command === "delayed_send_list" || command === "list_agent_providers" ? []
      : command === "plugin_manager_snapshot" ? { plugins: [], channels: [], diff: [], marketplace: [] } : {};
    return Promise.resolve(value as T);
  });
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const actions = new Proxy({}, { get: () => () => {} }) as MultiplexerActionsContextType;
  try {
    await act(async () => root.render(<MuxStateStoreContext.Provider value={createMuxStateStore(() => state)}>
      <MultiplexerActionsContext.Provider value={actions}>
        <TabContent tab={tab} isFocused isVisible onFocusTab={() => {}} onPtySpawned={() => {}}
          theme={{ name: "fixture", background: "#000", foreground: "#fff" } as ThemeConfig} />
      </MultiplexerActionsContext.Provider>
    </MuxStateStoreContext.Provider>));
    const chat = host.querySelector('[role="region"][aria-label="Tab chat: Main"]')!;
    const signInCard = () => chat.querySelector('[role="group"][aria-label="Sign in to Fixture"]');
    expect([...signInCard()!.querySelectorAll("button")].some(button => button.textContent === "Continue in browser")).toBe(true);
    await act(async () => publish({ payload: {
      tab_id: tab.id, session_binding_nonce: "general-binding",
      state: { state: "loaded", epoch: 1, auth: "signedIn", loginProviderName: "Fixture", loginMethods: ["browser"] },
    } }));
    expect(signInCard()).toBeNull();
    await act(async () => publish({ payload: {
      tab_id: tab.id, session_binding_nonce: "general-binding",
      state: { ...tab.agent_runtime_state!, authentication: { state: "authenticated", methods: [] } },
    } }));
    expect(signInCard()).not.toBeNull();
  } finally {
    await act(async () => root.unmount());
    host.remove();
    __setInvokeForTest(null);
    __setListenForTest(null);
  }
});

it("places recovery after the cached transcript with the diagnostic behind Details", async () => {
  const reason = "opaque: Interrupt completion deadline exceeded; runtime recovery required";
  const loaded: Tab = {
    id: "docked-notice", title: "Fixture", tab_type: "Terminal", cwd: null, pty_id: null, agent_type: "fixture",
    agent_session_id: "docked-session", session_binding_nonce: "docked-binding", boot_stage: "agent_ready",
    agent_session_model: "deepseek/deepseek-flash", agent_provider_resolved_model: "other/earlier-model",
    backing: { kind: "chat_backend", plugin_id: "fixture", session_id: "docked-session" },
    agent_runtime_state: { state: "loaded", epoch: 1 },
  };
  let tab = loaded;
  let state: ExtendedState = { workspaces: [], tabs: { [tab.id]: tab }, teams: [], active_workspace_id: null, focused_tab_id: tab.id,
    notifications: [], tabStatuses: {}, toasts: [], activeTabIds: {}, generation: 1, pendingCloses: [] };
  const store = createMuxStateStore(() => state);
  setAgentDescriptors([{
    schema_version: 1, id: "fixture", display_name: "Fixture", transport: "chat_backend", system_prompt_delivery: "external",
    available_models: [], notifications_schema: [], supports_lazy_skills: false, has_hooks: false,
    settings_schema: [{ kind: "toggle", key: "usage_tracking_enabled", label: "Usage tracking", default: true }],
    agent_runtime: { usage: true }, model_groups: [{ id: "deepseek", label: "DeepSeek" }],
  }], false);
  __setListenForTest(async () => () => {});
  __setInvokeForTest(<T,>(command: string, args?: Record<string, unknown>): Promise<T> => {
    if (command === "chat_start") return Promise.resolve({ text: "", offset: 1, start_offset: 0, pending: false,
      source_session_id: "docked-session", session_binding_nonce: "docked-binding", subscription_id: String(args?.subscriptionId),
      messages: [{ id: "docked-row", type: "assistant", content: "opaque retained reply" }] } as T);
    const value = command === "provider_model_usage" ? [{ group: "deepseek", spend_remaining_cents: 917 }]
      : command === "delayed_send_list" || command === "list_agent_providers" ? []
      : command === "plugin_manager_snapshot" ? { plugins: [], channels: [], diff: [], marketplace: [] } : {};
    return Promise.resolve(value as T);
  });
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  const actions = new Proxy({}, { get: () => () => {} }) as MultiplexerActionsContextType;
  const render = () => act(async () => root.render(
    <MuxStateStoreContext.Provider value={store}><MultiplexerActionsContext.Provider value={actions}>
      <TabContent tab={tab} isFocused isVisible onFocusTab={() => {}} onPtySpawned={() => {}}
        theme={{ name: "fixture", background: "#000", foreground: "#fff" } as ThemeConfig} />
    </MultiplexerActionsContext.Provider></MuxStateStoreContext.Provider>,
  ));
  try {
    await render();
    const balance = 'button[aria-label="DeepSeek balance, $9.17 left"]';
    expect(host.querySelector(`[data-testid="tab-top-bar"] ${balance}`)?.textContent).toBe("$9.17");
    const composerStatus = host.querySelector('[data-testid="agent-state"]');
    expect(composerStatus).not.toBeNull();
    expect(composerStatus?.textContent).not.toContain("$9.17");
    expect(host.querySelectorAll(balance)).toHaveLength(1);
    tab = { ...loaded, agent_runtime_state: { state: "degraded", epoch: 2, failureCode: "provider_refused", reason } };
    state = { ...state, tabs: { [tab.id]: tab } };
    await act(async () => store.notify());
    await render();
    const log = host.querySelector('[role="log"]')!;
    const notice = log.querySelector('[data-testid="runtime-chat-notice"]')!;
    expect(notice.querySelector('[role="status"]')?.textContent).toBe("The agent runtime stopped responding.");
    expect(log.textContent).not.toContain(reason);
    const details = [...notice.querySelectorAll("button")].find(button => button.textContent === "Details")!;
    expect(details.getAttribute("aria-expanded")).toBe("false");
    await act(async () => details.click());
    expect(notice.textContent).toContain(reason);
    const message = log.querySelector('[data-testid="chat-message"]')!;
    expect(message.compareDocumentPosition(notice) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(host.querySelector('[data-testid="session-status-row"] [role="status"]')).toBeNull();
  } finally {
    await act(async () => root.unmount()); host.remove();
    setAgentDescriptors([], false);
    __setInvokeForTest(null); __setListenForTest(null);
  }
});

it.each([
  ["a kept conversation", [{ id: "kept-row", type: "assistant", content: "opaque retained reply" }]],
  ["an empty conversation", []],
])("keeps a launched session readable with a transcript Restart after its relaunch failed, with %s", async (_name, messages) => {
  const reason = "opaque: Interrupt completion deadline exceeded; runtime recovery required";
  const tab: Tab = {
    id: `relaunch-failed-${messages.length}`, title: "Fixture", tab_type: "Terminal", cwd: null, pty_id: null, agent_type: "fixture",
    agent_session_id: "kept-session", session_binding_nonce: "kept-binding", boot_stage: "spawn_failed", boot_failure_reason: reason,
    backing: { kind: "chat_backend", plugin_id: "fixture", session_id: "kept-session" },
    agent_runtime_state: { state: "degraded", epoch: 2, failureCode: "provider_refused", reason },
  };
  const state: ExtendedState = { workspaces: [], tabs: { [tab.id]: tab }, teams: [], active_workspace_id: null, focused_tab_id: tab.id,
    notifications: [], tabStatuses: {}, toasts: [], activeTabIds: {}, generation: 1, pendingCloses: [] };
  const restarts: unknown[] = [];
  __setListenForTest(async () => () => {});
  __setInvokeForTest(<T,>(command: string, args?: Record<string, unknown>): Promise<T> => {
    if (command === "restart_agent_tab") restarts.push(args);
    if (command === "chat_start") return Promise.resolve({ text: "", offset: messages.length, start_offset: 0, pending: false,
      source_session_id: "kept-session", session_binding_nonce: "kept-binding", subscription_id: String(args?.subscriptionId), messages } as T);
    const value = command === "delayed_send_list" || command === "list_agent_providers" ? []
      : command === "plugin_manager_snapshot" ? { plugins: [], channels: [], diff: [], marketplace: [] } : {};
    return Promise.resolve(value as T);
  });
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  const actions = new Proxy({}, { get: () => () => {} }) as MultiplexerActionsContextType;
  try {
    await act(async () => root.render(<MuxStateStoreContext.Provider value={createMuxStateStore(() => state)}>
      <MultiplexerActionsContext.Provider value={actions}>
        <TabContent tab={tab} isFocused isVisible onFocusTab={() => {}} onPtySpawned={() => {}}
          theme={{ name: "fixture", background: "#000", foreground: "#fff" } as ThemeConfig} />
      </MultiplexerActionsContext.Provider>
    </MuxStateStoreContext.Provider>));
    expect(host.querySelector('[data-testid="chat-launch-failed"]')).toBeNull();
    const log = host.querySelector('[role="log"][aria-label="Conversation"]');
    expect(log).not.toBeNull();
    expect(log!.closest("[inert]")).toBeNull();
    expect(host.querySelectorAll('[data-testid="chat-message"]').length).toBe(messages.length);
    expect(host.querySelector('textarea[aria-label="Message composer"]')).not.toBeNull();
    const notice = log!.querySelector('[data-testid="runtime-chat-notice"]')!;
    const restart = Array.from(host.querySelectorAll("button")).filter(button => button.textContent === "Restart the agent");
    expect(restart.length).toBe(1);
    expect(notice.contains(restart[0])).toBe(true);
    await act(async () => restart[0].click());
    expect(restarts).toEqual([{ tabId: tab.id }]);
  } finally {
    await act(async () => root.unmount()); host.remove();
    __setInvokeForTest(null); __setListenForTest(null);
  }
});

it.each(["agent_launching", "spawn_failed"] as const)("keeps saved chat visible beside typed load recovery at %s and starts fresh in place", async bootStage => {
  const viewportHeight = vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(1000);
  const tab: Tab = {
    id:"load-refused-chat", title:"Fixture", tab_type:"Terminal", cwd:null, pty_id:null,
    agent_type:"fixture", agent_session_id:"saved-thread", session_binding_nonce:"saved-binding",
    backing:{kind:"chat_backend",plugin_id:"fixture",session_id:"saved-thread"},
    boot_stage:bootStage, boot_failure_reason:"opaque provider detail",
    agent_runtime_state:{state:"degraded",epoch:1,failureCode:"session_load_refused",reason:"opaque provider detail"},
  };
  const state: ExtendedState = { workspaces:[],tabs:{[tab.id]:tab},teams:[],active_workspace_id:null,
    focused_tab_id:tab.id,notifications:[],tabStatuses:{},toasts:[],activeTabIds:{},generation:1,pendingCloses:[] };
  const requests: Array<{command:string;args?:Record<string,unknown>}> = [];
  const saved = ["user", "status", "system", "user", "status", "user", "status", "system"]
    .map((type, seq) => ({id:`saved-row-${seq}`, type, content:`opaque saved turn ${seq}`, seq}));
  __setListenForTest(async () => () => {});
  __setInvokeForTest(async <T,>(command:string,args?:Record<string,unknown>) => {
    requests.push({command,args});
    const value = command === "chat_start" ? {messages:saved,
      pending:false,text:"",offset:8,offset_kind:"items",source_session_id:"saved-thread",session_binding_nonce:"saved-binding"}
      : command === "delayed_send_list" || command === "list_agent_providers" ? []
      : command === "plugin_manager_snapshot" ? {plugins:[],channels:[],diff:[],marketplace:[]} : {};
    return value as T;
  });
  const host=document.createElement("div");document.body.append(host);
  const root=createRoot(host);
  const actions=new Proxy({}, {get:()=>()=>{}}) as MultiplexerActionsContextType;
  try {
    await act(async () => root.render(<MuxStateStoreContext.Provider value={createMuxStateStore(()=>state)}>
      <MultiplexerActionsContext.Provider value={actions}>
        <TabContent tab={tab} isFocused isVisible onFocusTab={()=>{}} onPtySpawned={()=>{}}
          theme={{name:"fixture",background:"#000",foreground:"#fff"} as ThemeConfig}/>
      </MultiplexerActionsContext.Provider>
    </MuxStateStoreContext.Provider>));
    const conversation=host.querySelector('[role="log"][aria-label="Conversation"]');
    expect(conversation?.textContent).toContain("opaque saved turn");
    for (const row of saved) expect(conversation?.textContent).toContain(row.content);
    expect(conversation?.closest("[inert]")).toBeNull();
    const start=conversation?.querySelector<HTMLButtonElement>('button[aria-label="Start new session"]');
    expect(start).not.toBeNull();
    expect(host.querySelector('[role="progressbar"]')).toBeNull();
    expect(conversation?.querySelector("details pre")?.textContent).toBe("opaque provider detail");
    await act(async()=>start!.click());
    expect(requests.filter(request=>request.command==="restart_agent_tab")).toEqual([
      {command:"restart_agent_tab",args:{tabId:tab.id,fresh:true}},
    ]);
    expect(conversation?.textContent).toContain("opaque saved turn");
  } finally {
    await act(async()=>root.unmount());host.remove();__setInvokeForTest(null);__setListenForTest(null);
    viewportHeight.mockRestore();
  }
});

it("a conversation the harness lost is one notice whose one action starts a new conversation", async () => {
  const detail = "opaque-path → HTTP 409: ConversationMissing: opaque provider detail";
  const tab: Tab = {
    id:"lost-thread-chat", title:"Fixture", tab_type:"Terminal", cwd:null, pty_id:null,
    agent_type:"fixture", agent_session_id:"lost-thread", session_binding_nonce:"lost-binding",
    backing:{kind:"chat_backend",plugin_id:"fixture",session_id:"lost-thread"}, boot_stage:"agent_launching",
    agent_runtime_state:{state:"degraded",epoch:1,failureCode:"thread_not_found",reason:"ConversationMissing: opaque provider detail"},
  };
  const state: ExtendedState = { workspaces:[],tabs:{[tab.id]:tab},teams:[],active_workspace_id:null,
    focused_tab_id:tab.id,notifications:[],tabStatuses:{},toasts:[],activeTabIds:{},generation:1,pendingCloses:[] };
  const requests: Array<{command:string;args?:Record<string,unknown>}> = [];
  __setListenForTest(async () => () => {});
  __setInvokeForTest(async <T,>(command:string,args?:Record<string,unknown>) => {
    requests.push({command,args});
    if (command === "chat_start") throw new Error(detail);
    const value = command === "delayed_send_list" || command === "list_agent_providers" ? []
      : command === "plugin_manager_snapshot" ? {plugins:[],channels:[],diff:[],marketplace:[]} : {};
    return value as T;
  });
  const host=document.createElement("div");document.body.append(host);
  const root=createRoot(host);
  const actions=new Proxy({}, {get:()=>()=>{}}) as MultiplexerActionsContextType;
  try {
    await act(async () => root.render(<MuxStateStoreContext.Provider value={createMuxStateStore(()=>state)}>
      <MultiplexerActionsContext.Provider value={actions}>
        <TabContent tab={tab} isFocused isVisible onFocusTab={()=>{}} onPtySpawned={()=>{}}
          theme={{name:"fixture",background:"#000",foreground:"#fff"} as ThemeConfig}/>
      </MultiplexerActionsContext.Provider>
    </MuxStateStoreContext.Provider>));
    await act(async () => {});
    expect(host.querySelector('[aria-label="Chat connection failed"]')).toBeNull();
    expect(host.querySelector('[aria-label="Runtime needs recovery"]')).toBeNull();
    expect(host.querySelector('button[aria-label="Restart the agent"]')).toBeNull();
    expect(host.querySelector('[data-testid="chat-retry"]')).toBeNull();
    expect(host.textContent?.split("The agent no longer has this conversation, so it can't continue.").length).toBe(2);
    const start=host.querySelectorAll<HTMLButtonElement>('button[aria-label="Start a new conversation"]');
    expect(start).toHaveLength(1);
    expect(host.querySelector("details")?.open).toBe(false);
    await act(async()=>start[0].click());
    expect(requests.filter(request=>request.command==="restart_agent_tab")).toEqual([
      {command:"restart_agent_tab",args:{tabId:tab.id,fresh:true}},
    ]);
  } finally {
    await act(async()=>root.unmount());host.remove();__setInvokeForTest(null);__setListenForTest(null);
  }
});

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
    expect(host.querySelector('[role="log"] [data-testid="runtime-chat-notice"]')).not.toBeNull();
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
    expect(host.querySelector('[data-testid="session-status-row"]')?.children.length).toBe(0);
    const waitingStatus = host.querySelector('[data-testid="tab-top-bar"] [role="status"]');
    expect(waitingStatus?.closest('[aria-hidden="true"]')).not.toBeNull();
    await act(async () => {
      const event = new Event("animationstart", { bubbles: true });
      Object.defineProperty(event, "animationName", { value: "ct-presence-enter" });
      waitingStatus!.parentElement!.dispatchEvent(event);
    });
    const headerStatus = host.querySelector('[data-testid="tab-top-bar"] [role="status"]');
    expect(headerStatus?.closest('[aria-hidden="true"]')).toBeNull();
    expect(headerStatus?.getAttribute("aria-live")).toBe("polite");
    expect(headerStatus?.getAttribute("aria-label")).toBe("Restoring…");
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
    expect(host.querySelector('[data-testid="session-status-row"]')?.children.length).toBe(0);
    const settledHeaderStatus = host.querySelector('[data-testid="tab-top-bar"] [role="status"]');
    expect(settledHeaderStatus === null || settledHeaderStatus.closest('[aria-hidden="true"]') !== null).toBe(true);
    expect(host.querySelector("textarea")).toBe(composer);
    expect(host.textContent).toContain("persisted-fixture-content");
  } finally {
    await act(async () => root.unmount());
    host.remove();
    __setInvokeForTest(null);
    __setListenForTest(null);
  }
});

it("keeps restore failure notices in session-status-row without showing restoring in the header subtitle", async () => {
  const tab: Tab = {
    id: "wake-refused-fixture", title: "Fixture", tab_type: "Terminal", cwd: null, pty_id: null, frozen: true, agent_type: "fixture",
    slept_at_ms: 1000, agent_session_id: "saved", session_binding_nonce: "sleep-binding",
    backing: { kind: "chat_backend", plugin_id: "fixture", session_id: "saved" },
  };
  const state: ExtendedState = {
    workspaces: [], tabs: { [tab.id]: tab }, teams: [], active_workspace_id: null, focused_tab_id: tab.id,
    notifications: [], tabStatuses: {}, toasts: [], activeTabIds: {}, generation: 1, pendingCloses: [],
  };
  __setInvokeForTest(<T,>(command: string, args?: Record<string, unknown>): Promise<T> => {
    if (command === "wake_tab") {
      return Promise.reject(new PeerTabRelayError(403, "/api/mesh/relay-to", "Opaque wake refusal", "peer_relay_not_authorized"));
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
    expect(host.querySelector('[data-testid="tab-top-bar"] [role="status"]')).toBeNull();
    const statusRow = host.querySelector('[data-testid="session-status-row"]');
    expect(statusRow?.children.length).toBe(1);
    expect(statusRow?.querySelector('section[aria-label="Session status"] [role="status"]')).not.toBeNull();
    expect(statusRow?.textContent).toContain("Opaque wake refusal");
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
  __setInvokeForTest(async <T,>(command: string, args?: Record<string, unknown>) => {
    if (command === "chat_start") {
      const pending = state.tabs[tab.id].frozen;
      return { messages: [], pending, text: "", start_offset: 0, offset: 0,
        source_session_id: "own-session", session_binding_nonce: pending ? "old-binding" : "restored-binding",
        subscription_id: args?.subscriptionId } as T;
    }
    const value = command === "delayed_send_list" || command === "list_agent_providers" ? []
      : command === "plugin_manager_snapshot" ? { plugins: [], channels: [], diff: [], marketplace: [] }
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
    const composer = host.querySelector<HTMLTextAreaElement>('textarea[aria-label="Message composer"]');
    expect(composer?.disabled).toBe(false);
    expect(composer?.closest("[inert]")).toBeNull();
    expect(host.querySelector('[aria-label="Tab chat: Grok"]')?.getAttribute("data-chat-ready")).toBe("false");

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
    expect(host.querySelector('[aria-label="Tab chat: Grok"]')?.getAttribute("data-chat-ready")).toBe("true");
    expect([...host.querySelectorAll('[role="status"]')].some(status => status.textContent === "No earlier messages.")).toBe(false);
    expect(host.querySelector('[data-testid="chat-load-older"]')).toBeNull();
    expect(host.querySelector('textarea[aria-label="Message composer"]')).toBe(composer);
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
    await act(async () => reconnectTabSession(tab.id));
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
    expect(ownSurface().querySelector('[data-testid="runtime-chat-notice"]')).toBeNull();
    expect(ownSurface().querySelector("textarea")).toBe(composer);
    tab = { ...tab, agent_runtime_state: { state: "degraded", epoch: 3, reason: "ProcessExited: opaque" } };
    state = { ...state, tabs: { ...state.tabs, [tab.id]: tab } };
    await act(async () => store.notify());
    await render();
    expect(ownSurface().querySelector('[role="log"] [data-testid="runtime-chat-notice"]')).not.toBeNull();
  } finally {
    await act(async () => root.unmount()); host.remove();
    __setInvokeForTest(null); __setListenForTest(null);
  }
  expect(subscriptions.size).toBe(0);
});
