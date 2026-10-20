import { act, useRef } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import type { NormalizedChatMessage } from "@codeterm/chat-core";
import { __setInvokeForTest, __setListenForTest } from "../../lib/bridge";
import { setAgentDescriptors } from "../../lib/agentDescriptorCache";
import type { ProviderDescriptor, Tab, ThemeConfig } from "../../lib/types";
import { MuxStateStoreContext, MultiplexerActionsContext, type MultiplexerActionsContextType } from "../../store/multiplexer/context";
import { createMuxStateStore } from "../../store/multiplexer/selectorStore";
import type { ExtendedState } from "../../store/multiplexer/tree";
import { TabContent } from "./TabContent";
import { clearTranscriptWindows } from "./transcriptStore";
import { insertIntoComposer } from "./composerInsertion";

vi.mock("../../hooks/useAgentProviders", () => ({ useAgentProviders: () => ({ providers: [], loading: false, error: null }) }));
vi.mock("../../hooks/terminal/useXterm", () => ({ useXterm: () => ({
  termRef: useRef(null), fitAddonRef: useRef(null), searchAddonRef: useRef(null), termReady: false, bellFlash: false,
}) }));
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

it("same-tab streaming assistant growth preserves immediate composer edits without background root renders", async () => {
  const tabId = "streaming-input";
  const descriptor: ProviderDescriptor = {
    schema_version: 1, id: "stream-fixture", display_name: "Stream fixture", transport: "pty", system_prompt_delivery: "external",
    available_models: [], settings_schema: [], notifications_schema: [], supports_lazy_skills: false, has_hooks: false,
    supports_chat_ui: true, supports_prompt_answers: true, agent_runtime: { goals: { actions: ["read"] } },
  };
  const tab: Tab = { id: tabId, title: "Fixture", tab_type: "Terminal", cwd: null, pty_id: "pty-fixture", backing: { kind: "pty" },
    agent_type: descriptor.id, agent_session_id: "thread", session_binding_nonce: "binding", boot_stage: "agent_ready",
    agent_runtime_state: { state: "loaded", epoch: 1 }, agent_runtime_questions: [],
    agent_state: { state: "working", phase: "streaming", status_line: "Fixture working" },
  };
  let state: ExtendedState = { workspaces: [], tabs: { [tabId]: tab }, teams: [], active_workspace_id: null,
    focused_tab_id: tabId, notifications: [], tabStatuses: {}, toasts: [], activeTabIds: {}, generation: 1, pendingCloses: [] };
  const store = createMuxStateStore(() => state);
  const listeners = new Map<string, Set<(event: { payload: unknown }) => void>>();
  const calls: Array<{ command: string; args?: Record<string, unknown> }> = [];
  const message = (content: string): NormalizedChatMessage => ({ id: "answer", type: "assistant", content, timestamp: "1700000001000", seq: 2 });
  const intent: NormalizedChatMessage = { id: "goal", type: "assistant", content: "", timestamp: "1700000000000", seq: 1,
    sessionIntent: { title: "Fixture", goal: "Initial objective", goalStatus: "active" } };
  let goal = intent.sessionIntent;
  let finishDelayed!: (value: unknown) => void;
  const delayed = new Promise<unknown>(resolve => { finishDelayed = resolve; });
  __setInvokeForTest(async <T,>(command: string, args?: Record<string, unknown>): Promise<T> => {
    calls.push({ command, args });
    if (command === "delayed_send_list") return await delayed as T;
    if (["list_agent_providers", "cron_list", "list_shell_profiles"].includes(command)) return [] as T;
    if (command === "chat_send_status") return [] as T;
    if (command === "chat_start") return { messages: [intent, message("start")], pending: false, text: "", start_offset: 0, offset: 2,
      source_session_id: "thread", session_binding_nonce: "binding", subscription_id: args?.subscriptionId } as T;
    if (command === "plugin_manager_snapshot") return { plugins: [], channels: [], diff: [], marketplace: [] } as T;
    if (command === "plugin_tab_menu") return { items: [], receiverTargets: {} } as T;
    if (command === "agent_runtime_goal") return { supported: true, binding: { threadId: "thread", bindingNonce: "binding", epoch: 1 }, intent: goal, actions: ["read"] } as T;
    if (command === "agent_runtime_retry") return { state: "loaded", epoch: 2 } as T;
    return {} as T;
  });
  __setListenForTest(async (name, handler) => {
    const handlers = listeners.get(name) ?? new Set();
    handlers.add(handler as (event: { payload: unknown }) => void);
    listeners.set(name, handlers);
    return () => { handlers.delete(handler as (event: { payload: unknown }) => void); };
  });
  let nextFrame = 0;
  const frames = new Map<number, FrameRequestCallback>();
  vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => { frames.set(++nextFrame, callback); return nextFrame; });
  vi.stubGlobal("cancelAnimationFrame", (handle: number) => { frames.delete(handle); });
  const frame = () => act(async () => {
    const current = [...frames.values()]; frames.clear();
    for (const callback of current) callback(nextFrame);
  });
  const emit = (name: string, payload: unknown) => act(async () => {
    expect(listeners.get(name)?.size).toBeGreaterThan(0);
    for (const handler of listeners.get(name) ?? []) handler({ payload });
  });
  window.__composerPerf = { inputMarks: [], paintLatencies: [], renderCounts: {}, compositionEvents: [], ignoredKeys: [] };
  setAgentDescriptors([descriptor], false);
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  const noop = () => {};
  const actions = new Proxy({}, { get: () => noop }) as MultiplexerActionsContextType;
  const edit = (editor: HTMLTextAreaElement, value: string) => act(async () => {
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(editor, value);
    editor.dispatchEvent(new Event("input", { bubbles: true }));
  });
  try {
    await act(async () => root.render(<MuxStateStoreContext.Provider value={store}>
      <MultiplexerActionsContext.Provider value={actions}>
        <TabContent tab={tab} isVisible isFocused onFocusTab={() => {}} onPtySpawned={() => {}}
          theme={{ name: "fixture", background: "#000", foreground: "#fff" } as ThemeConfig} />
      </MultiplexerActionsContext.Provider>
    </MuxStateStoreContext.Provider>));
    await frame();
    const editor = host.querySelector<HTMLTextAreaElement>('textarea[aria-label="Message composer"]')!;
    expect(editor).not.toBeNull();
    expect(host.querySelector('[data-testid="terminal-tab"]')).not.toBeNull();
    editor.focus();
    const initialRenders = window.__composerPerf.renderCounts[tabId];
    let growing = "start";
    for (const mode of ["transcript", "snapshot", "combined"]) for (let index = 1; index <= 10; index++) {
      const beforeBackground = window.__composerPerf.renderCounts[tabId];
      if (mode !== "snapshot") {
        growing += " growth";
        await emit(`tab-chat-msg-${tabId}`, { tab_id: tabId, messages: [message(growing)],
          source_session_id: "thread", session_binding_nonce: "binding" });
      }
      if (mode !== "transcript") await act(async () => {
        state = { ...structuredClone(state), generation: state.generation + 1 }; store.notify();
      });
      await frame();
      expect(window.__composerPerf.renderCounts[tabId], `${mode} update ${index}`).toBe(beforeBackground);
      expect(host.textContent).toContain(growing);
      await edit(editor, `local edit ${mode} ${index}`);
      expect(editor.value).toBe(`local edit ${mode} ${index}`);
      expect(document.activeElement).toBe(editor);
      expect(host.querySelector('textarea[aria-label="Message composer"]')).toBe(editor);
    }
    expect(window.__composerPerf.renderCounts[tabId]).toBeGreaterThan(initialRenders);
    await act(async () => insertIntoComposer(tabId, [{ kind: "document", path: "/fixture/kept.md" }]));
    const capsule = host.querySelector('[data-testid="composer-capsule-mirror"]')?.textContent;
    expect(capsule).toContain("kept.md");
    await act(async () => editor.dispatchEvent(new Event("compositionstart", { bubbles: true })));
    await edit(editor, `${editor.value} 文`);
    const composed = editor.value;
    growing += " composition growth";
    await emit(`tab-chat-msg-${tabId}`, { tab_id: tabId, messages: [message(growing)], source_session_id: "thread", session_binding_nonce: "binding" });
    await frame();
    expect(editor.value).toBe(composed);
    await act(async () => {
      const enter = new KeyboardEvent("keydown", { key: "Enter", code: "Enter", bubbles: true, cancelable: true });
      Object.defineProperty(enter, "isComposing", { value: true });
      editor.dispatchEvent(enter);
      editor.dispatchEvent(new Event("compositionend", { bubbles: true }));
    });
    expect(editor.value).toBe(composed);
    expect(host.querySelector('[data-testid="composer-capsule-mirror"]')?.textContent).toContain("kept.md");
    expect(host.querySelector('textarea[aria-label="Message composer"]')).toBe(editor);

    await act(async () => host.querySelector<HTMLButtonElement>('button[aria-label="Session goal"]')!.click());
    expect(host.querySelector('[role="dialog"][aria-label="Session goal"]')?.textContent).toContain("Initial objective");
    goal = { title: "Updated title", goal: "Updated objective", goalStatus: "paused" };
    await emit(`tab-chat-msg-${tabId}`, { tab_id: tabId, messages: [{ ...intent, sessionIntent: goal }], source_session_id: "thread", session_binding_nonce: "binding" });
    await frame();
    const goalDialog = host.querySelector('[role="dialog"][aria-label="Session goal"]');
    expect(goalDialog?.textContent).toContain("Updated objective");
    expect(goalDialog?.querySelector('[role="status"][aria-label="Goal status"]')?.textContent).toBe("Paused");

    await act(async () => {
      state = { ...state, tabs: { [tabId]: { ...state.tabs[tabId], pending_prompt: { question: "Fixture decision", options: [{ label: "Choose fixture", value: "one" }] } } } };
      store.notify();
    });
    expect(host.querySelector('[role="group"][aria-label="Fixture decision"]')).not.toBeNull();
    expect(editor.value).toBe(composed);
    await edit(editor, "/fi");
    const commandData = { commands: [{ name: "fixture", description: "opaque", argument_hint: null }], models: null,
      agents: null, account: null, current_permission_mode: null, fast_mode_state: null, output_style: null };
    await act(async () => {
      state = { ...state, tabs: { [tabId]: { ...state.tabs[tabId], provider_signals: { auth: null, api_error: null, permission_mode: null,
        capabilities: { status: "ready", data: commandData } } } } };
      store.notify();
    });
    expect(document.querySelector('[role="option"][data-slash-command="/fixture"]')).not.toBeNull();
    await act(async () => {
      state = { ...state, tabs: { [tabId]: { ...state.tabs[tabId], provider_signals: { auth: null, api_error: null, permission_mode: null, capabilities: { status: "ready",
        data: { ...commandData, commands: [{ name: "final", description: "changed", argument_hint: null }] } } } } } };
      store.notify();
    });
    expect(document.querySelector('[role="option"][data-slash-command="/fixture"]')).toBeNull();
    expect(document.querySelector('[role="option"][data-slash-command="/final"]')).not.toBeNull();
    expect(editor.value).toBe("/fi");
    await edit(editor, composed);
    await emit("agent-runtime-state", { tab_id: tabId, session_binding_nonce: "binding", state: { state: "degraded", epoch: 1, reason: "Fixture failure" } });
    const runtimeBadge = host.querySelector<HTMLButtonElement>('button[aria-label="Agent runtime unavailable"]')!;
    expect(runtimeBadge).not.toBeNull();
    await act(async () => runtimeBadge.click());
    const retry = [...document.querySelectorAll<HTMLButtonElement>("button")].find(button => button.textContent === "Retry")!;
    expect(retry).toBeDefined();
    await act(async () => retry.click());
    expect(calls.filter(call => call.command === "agent_runtime_retry")).toEqual([{ command: "agent_runtime_retry", args: { tabId } }]);
    expect(host.querySelector('button[aria-label="Agent runtime unavailable"]')).toBeNull();
    expect(editor.value).toBe(composed);
    expect(host.textContent).toContain(growing);
    expect([...host.querySelectorAll('[data-testid="chat-message"]')].filter(row => row.textContent?.includes(growing))).toHaveLength(1);
    await emit(`tab-chat-msg-${tabId}`, { tab_id: tabId, messages: [message("Foreign binding content")], source_session_id: "thread", session_binding_nonce: "foreign" });
    await frame();
    expect(host.textContent).not.toContain("Foreign binding content");
    expect(host.textContent).toContain(growing);
    expect(host.querySelector('textarea[aria-label="Message composer"]')).toBe(editor);
    expect(editor.value).toBe(composed);
    expect(state.tabs[tabId].agent_session_id).toBe("thread");
    expect(state.tabs[tabId].session_binding_nonce).toBe("binding");
    expect(calls.filter(call => call.command === "chat_start")).toHaveLength(1);
    expect(listeners.get(`tab-chat-msg-${tabId}`)?.size).toBe(1);
    expect(calls.filter(call => ["chat_send_text", "write_pty_string", "wake_tab", "chat_stop", "mux_respond_prompt"].includes(call.command))).toEqual([]);
  } finally {
    finishDelayed([]);
    await act(async () => root.unmount()); host.remove();
    clearTranscriptWindows(); setAgentDescriptors([], false);
    __setInvokeForTest(null); __setListenForTest(null);
    delete window.__composerPerf; vi.unstubAllGlobals();
  }
});
