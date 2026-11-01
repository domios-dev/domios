import { act, useRef } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import type { ProviderDescriptor, Tab, ThemeConfig } from "../../lib/types";
import type { UsePtyConnectionOptions } from "../../hooks/terminal/usePtyConnection";
import { __setInvokeForTest, __setListenForTest } from "../../lib/bridge";
import { setAgentDescriptors } from "../../lib/agentDescriptorCache";
import { setClientTabUiMode } from "../../lib/clientTabUiMode";
import { MuxStateStoreContext, MultiplexerActionsContext, type MultiplexerActionsContextType } from "../../store/multiplexer/context";
import { createMuxStateStore } from "../../store/multiplexer/selectorStore";
import type { ExtendedState } from "../../store/multiplexer/tree";
import { TabContent } from "./TabContent";
import { clearTranscriptWindows } from "./transcriptStore";
import { MemoryChatSendRepository } from "./chatSendRepository.testSupport";
import { __setChatSendRepositoryForTest, readChatSends } from "./chatSendStore";

const lifecycle = vi.hoisted(() => ({ current: null as UsePtyConnectionOptions | null }));
vi.mock("../../hooks/useAgentProviders", () => ({ useAgentProviders: () => ({ providers: [], loading: false, error: null }) }));
vi.mock("../../hooks/terminal", async importOriginal => ({
  ...await importOriginal<typeof import("../../hooks/terminal")>(),
  useXterm: () => ({ termRef: useRef(null), fitAddonRef: useRef(null), searchAddonRef: useRef(null), termReady: false, bellFlash: false }),
  usePtyConnection: (options: UsePtyConnectionOptions) => { lifecycle.current = options; return { writePty: () => {} }; },
}));
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

async function mount(chat: boolean, protectedTab = false, sessionId: string | null = null) {
  const tabId = `body-ready-${chat ? "chat" : "terminal"}-${protectedTab}-${sessionId ?? "fresh"}`;
  const descriptor: ProviderDescriptor = {
    schema_version: 1, id: "body-ready-fixture", display_name: "Fixture", transport: "pty", system_prompt_delivery: "external",
    available_models: [], settings_schema: [], notifications_schema: [], supports_lazy_skills: false, has_hooks: false,
    supports_chat_ui: chat, supports_prompt_answers: true,
  };
  let tab: Tab = { id: tabId, title: "Fixture", tab_type: "Terminal", cwd: null, pty_id: null, backing: { kind: "pty" },
    agent_type: descriptor.id, protected: protectedTab, agent_session_id: sessionId, session_binding_nonce: "own-nonce",
    frozen: true, agent_booting: false, slept_at_ms: null, boot_stage: "pty_starting" };
  let state: ExtendedState = { workspaces: [], tabs: { [tabId]: tab }, teams: [], active_workspace_id: null,
    focused_tab_id: tabId, notifications: [], tabStatuses: {}, toasts: [], activeTabIds: {}, generation: 1, pendingCloses: [] };
  const store = createMuxStateStore(() => state);
  const commands: string[] = [];
  __setChatSendRepositoryForTest(new MemoryChatSendRepository());
  __setInvokeForTest(async <T,>(command: string, args?: Record<string, unknown>): Promise<T> => {
    commands.push(command);
    if (command === "chat_start") return { source_session_id: tab.agent_session_id, session_binding_nonce: tab.session_binding_nonce,
      subscription_id: args?.subscriptionId, pending: true, messages: [], text: "", offset: 0 } as T;
    if (["delayed_send_list", "system_heartbeat_list", "chat_send_status", "chat_send_dismissals", "chat_send_admissions", "list_agent_providers"].includes(command)) return [] as T;
    if (command === "plugin_manager_snapshot") return { plugins: [], channels: [], diff: [], marketplace: [] } as T;
    if (command === "plugin_tab_menu") return { items: [], receiverTargets: {} } as T;
    if (command === "chat_send_text") return { send_id: args?.clientToken, correlationId: args?.clientToken,
      tab_id: tabId, state: "written", error: null, ledger_id: "fixture", sequence: 1, retained_from: 0 } as T;
    return {} as T;
  });
  __setListenForTest(async () => () => {});
  setAgentDescriptors([descriptor], false);
  setClientTabUiMode(tabId, chat ? "chat" : "terminal");
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  const actions = new Proxy({}, { get: () => () => {} }) as MultiplexerActionsContextType;
  const view = () => <MuxStateStoreContext.Provider value={store}>
    <MultiplexerActionsContext.Provider value={actions}>
      <TabContent tab={tab} isVisible isFocused onFocusTab={() => {}} onPtySpawned={() => {}}
        theme={{ name: "fixture", background: "#000", foreground: "#fff" } as ThemeConfig} />
    </MultiplexerActionsContext.Provider>
  </MuxStateStoreContext.Provider>;
  await act(async () => root.render(view()));
  return { host, tabId, commands,
    async patch(patch: Partial<Tab>) {
      await act(async () => {
        tab = { ...tab, ...patch };
        state = { ...state, tabs: { [tabId]: tab }, generation: state.generation + 1 };
        store.notify(); root.render(view());
      });
    },
    async dispose() {
      await act(async () => root.unmount()); host.remove();
      __setInvokeForTest(null); __setListenForTest(null); __setChatSendRepositoryForTest(null);
      clearTranscriptWindows(); setAgentDescriptors([], false); lifecycle.current = null;
    },
  };
}

const readyLaunch = { pty_id: "own-pty", binding_nonce: "own-nonce", reason: "opaque",
  awaiting_binding_ack: true, first_turn_input_ready: true, first_turn_payload_written: false };
const enabledComposer = (host: HTMLElement) => [...host.querySelectorAll<HTMLTextAreaElement>('[data-testid="chat-composer"]')]
  .find(editor => !editor.disabled && !editor.closest('[inert], [aria-hidden="true"]'));

it.each([
  [false, null], [true, null], [false, "own-session"], [true, "own-session"],
] as const)("reveals the real first-turn composer for General=%s session=%s without fabricating a snapshot", async (general, session) => {
  const fixture = await mount(true, general, session);
  try {
    expect(enabledComposer(fixture.host)).toBeUndefined();
    await fixture.patch({ frozen: false, pty_id: "own-pty", boot_stage: "provider_ready", provisional_launch: readyLaunch });
    const editor = enabledComposer(fixture.host);
    expect(editor).toBeDefined();
    const region = fixture.host.querySelector('[data-testid="tab-session-chat"]');
    expect(region?.getAttribute("data-snapshot-ready")).toBe("false");
    expect(region?.getAttribute("data-chat-ready")).toBe("true");
    const text = "opaque first local turn";
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(editor, text);
      editor!.dispatchEvent(new Event("input", { bubbles: true }));
    });
    const send = fixture.host.querySelector<HTMLButtonElement>('[data-testid="composer-send-button"]')!;
    expect(send.disabled).toBe(false);
    await act(async () => send.dispatchEvent(new PointerEvent("pointerup", { bubbles: true, button: 0, pointerType: "mouse" })));
    expect(fixture.commands.filter(command => command === "chat_send_text")).toHaveLength(1);
    expect(readChatSends(fixture.tabId).some(message => message.content === text)).toBe(true);
    await fixture.patch({ provisional_launch: { ...readyLaunch, first_turn_input_ready: false, first_turn_payload_written: true } });
    expect(enabledComposer(fixture.host)).toBeDefined();
    expect(fixture.host.querySelector('[data-testid="tab-session-chat"]')?.getAttribute("data-snapshot-ready")).toBe("false");
    expect([...fixture.host.querySelectorAll("[data-chat-window-row]")].filter(row => row.textContent?.includes(text))).toHaveLength(1);
  } finally { await fixture.dispose(); }
});

it("keeps stale first-turn readiness, locked tabs and launch errors from revealing an active composer", async () => {
  const fixture = await mount(true);
  try {
    await fixture.patch({ frozen: false, pty_id: "own-pty", boot_stage: "provider_ready",
      provisional_launch: { ...readyLaunch, binding_nonce: "foreign-nonce" } });
    expect(enabledComposer(fixture.host)).toBeUndefined();
    await fixture.patch({ locked: true, provisional_launch: readyLaunch });
    expect(enabledComposer(fixture.host)).toBeUndefined();
    await fixture.patch({ locked: false, boot_stage: "spawn_failed", boot_failure_reason: "opaque launch error" });
    expect(enabledComposer(fixture.host)).toBeUndefined();
  } finally { await fixture.dispose(); }
});

it("reveals an empty real TerminalTab only after its current frame and connection acknowledgements", async () => {
  const fixture = await mount(false);
  try {
    await fixture.patch({ frozen: false, pty_id: "own-pty", boot_stage: "agent_ready" });
    const frame = fixture.host.querySelector<HTMLElement>('[data-testid="terminal-frame"]')!;
    expect(frame).not.toBeNull();
    const rows = document.createElement("div"); rows.className = "xterm-rows"; rows.textContent = "prior hidden text"; frame.append(rows);
    await act(async () => {});
    expect(frame.style.visibility).toBe("hidden");
    expect(frame.closest('[inert], [aria-hidden="true"]')).not.toBeNull();
    await act(async () => {
      lifecycle.current!.onConnectionStateChange?.(true);
      lifecycle.current!.onFrameReady?.("foreign-pty");
    });
    expect(frame.getAttribute("data-terminal-ready")).toBe("false");
    expect(frame.closest('[inert], [aria-hidden="true"]')).not.toBeNull();
    rows.textContent = "";
    await act(async () => lifecycle.current!.onFrameReady?.("own-pty"));
    expect(frame.getAttribute("data-terminal-ready")).toBe("true");
    expect(frame.closest('[inert], [aria-hidden="true"]')).toBeNull();
    expect(rows.textContent).toBe("");
  } finally { await fixture.dispose(); }
});
