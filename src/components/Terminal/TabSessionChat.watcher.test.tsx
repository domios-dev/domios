// Run: npx tsx src/components/Terminal/TabSessionChat.watcher.test.tsx

import { act } from "react";
import { Window } from "happy-dom";
import { createRoot, type Root } from "react-dom/client";
import type { Tab } from "../../lib/types";
import { MultiplexerActionsContext, MuxStateStoreContext, type MultiplexerActionsContextType } from "../../store/multiplexer/context";
import { createMuxStateStore } from "../../store/multiplexer/selectorStore";
import type { ExtendedState } from "../../store/multiplexer/tree";

type TestFn = () => void | Promise<void>;
const tests: Array<[string, TestFn]> = [];

function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function setupDom() {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const window = new Window({ url: "http://localhost/" });
  Object.defineProperty(globalThis, "window", { value: window, configurable: true });
  Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
  Object.defineProperty(globalThis, "navigator", { configurable: true, value: window.navigator });
  Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
  Object.defineProperty(globalThis, "HTMLTextAreaElement", { value: window.HTMLTextAreaElement, configurable: true });
  Object.defineProperty(globalThis, "SVGElement", { value: window.SVGElement, configurable: true });
  Object.defineProperty(globalThis, "Element", { value: window.Element, configurable: true });
  Object.defineProperty(globalThis, "Node", { value: window.Node, configurable: true });
  Object.defineProperty(globalThis, "Event", { value: window.Event, configurable: true });
  Object.defineProperty(globalThis, "CustomEvent", { value: window.CustomEvent, configurable: true });
  Object.defineProperty(globalThis, "requestAnimationFrame", {
    configurable: true,
    value: (cb: FrameRequestCallback) => {
      cb(0);
      return 0;
    },
  });
  Object.defineProperty(globalThis, "cancelAnimationFrame", {
    configurable: true,
    value: () => {},
  });
  class StubResizeObserver {
    observe(): void {}
    unobserve(): void {}
    disconnect(): void {}
  }
  Object.defineProperty(globalThis, "ResizeObserver", { configurable: true, value: StubResizeObserver });
  return window;
}

function tabBase(id: string, overrides: Partial<Tab> = {}): Tab {
  return {
    id,
    pty_id: null,
    tab_type: "Terminal",
    title: id,
    cwd: null,
    backing: { kind: "chat_backend", plugin_id: "lmstudio", session_id: "sess-1" },
    ...overrides,
  } as Tab;
}

function muxActions(
  overrides: Partial<MultiplexerActionsContextType> = {},
): MultiplexerActionsContextType {
  const noop = () => {};
  const noopAsync = async () => undefined;
  return {
    createWorkspace: noopAsync,
    setActiveWorkspace: noop,
    closeWorkspace: noop,
    renameWorkspace: noop,
    moveTab: noop,
    reorderTabs: noop,
    reorderWorkspaces: noop,
    createTab: noopAsync,
    closeTab: noop,
    reopenClosed: noop,
    reopenGroup: noop,
    restoreOrphaned: noop,
    setActiveTab: noop,
    selectTabByTab: noopAsync,
    setFocusedTab: noop,
    setTabPty: noop,
    addNotification: noop,
    upsertToast: noop,
    markRead: noop,
    markAllRead: noop,
    deleteNotification: noop,
    clearNotifications: noop,
    dismissToast: noop,
    setTabTitle: noop,
    setAgentType: noop,
    updateTabStatus: noop,
    spawnClaudeTab: noop,
    spawnAgentTab: async () => null,
    createOrchestrationPair: async () => {
      throw new Error("not implemented");
    },
    ...overrides,
  };
}

function muxStateFor(tabs: Record<string, Tab>): ExtendedState {
  return {
    workspaces: [],
    tabs,
    teams: [],
    active_workspace_id: null,
    focused_tab_id: Object.keys(tabs)[0] ?? null,
    notifications: [],
    tabStatuses: {},
    toasts: [],
    activeTabIds: {},
    generation: 1,
    pendingCloses: [],
  };
}

async function renderTabSessionChat(
  tabId: string,
  state: ExtendedState,
  chatStart: () => unknown = () => ({ text: "", offset: 0, messages: [] }),
): Promise<{
  host: HTMLDivElement;
  root: Root;
  commands: string[];
  emitChat: (payload: { tab_id: string; messages: Array<{ id: string; type: "assistant"; content: string; timestamp: string }>; source_session_id: string | null; session_binding_nonce: string | null }) => void;
}> {
  const { __setInvokeForTest, __setListenForTest } = await import("../../lib/bridge");
  const { TabSessionChat } = await import("./TabSessionChat");

  const commands: string[] = [];
  let chatListener: ((event: { payload: unknown }) => void) | null = null;
  __setInvokeForTest((async (cmd: string) => {
    commands.push(cmd);
    if (cmd === "chat_start") {
      return {
        source_session_id: state.tabs[tabId]?.agent_session_id ?? null,
        session_binding_nonce: state.tabs[tabId]?.session_binding_nonce ?? null,
        subscription_id: "test-subscription",
        ...(chatStart() as object),
      };
    }
    if (cmd === "delayed_send_list") return [];
    return {};
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
  }) as any);
  __setListenForTest(async (event, handler) => {
    if (event === `tab-chat-msg-${tabId}`) chatListener = handler as (event: { payload: unknown }) => void;
    return () => {
      if (event === `tab-chat-msg-${tabId}`) chatListener = null;
    };
  });

  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  const store = createMuxStateStore(() => state);
  await act(async () => {
    root.render(
      <MuxStateStoreContext.Provider value={store}>
        <MultiplexerActionsContext.Provider value={muxActions()}>
          <TabSessionChat
            tabId={tabId}
            chatBackend={state.tabs[tabId]?.backing?.kind === "chat_backend"}
            isVisible
          />
        </MultiplexerActionsContext.Provider>
      </MuxStateStoreContext.Provider>,
    );
  });
  await act(async () => {
    await Promise.resolve();
  });
  return {
    host,
    root,
    commands,
    emitChat: (payload) => chatListener?.({ payload }),
  };
}

const testWindow = setupDom();

test("store Tab action doubles preserve invocation contracts", async () => {
  const activations: Array<[string, string]> = [];
  const moves: Array<[string, string, string]> = [];
  const creates: Array<[string, string | undefined]> = [];
  const closes: Array<[string, string]> = [];
  const claudeSpawns: Array<[string, boolean, string | undefined]> = [];
  const agentSpawns: Array<[string, string, boolean, string | undefined, string | undefined]> = [];
  const createdTabResult = { id: "created-tab" } as Awaited<
    ReturnType<MultiplexerActionsContextType["createTab"]>
  >;
  const actions = muxActions({
    moveTab: (tabId, fromWorkspaceId, toWorkspaceId) => {
      moves.push([tabId, fromWorkspaceId, toWorkspaceId]);
    },
    createTab: async (workspaceId, cwd) => {
      creates.push([workspaceId, cwd]);
      return createdTabResult;
    },
    closeTab: (workspaceId, tabId) => {
      closes.push([workspaceId, tabId]);
    },
    setActiveTab: (workspaceId, tabId) => {
      activations.push([workspaceId, tabId]);
    },
    spawnClaudeTab: (workspaceId, skipPermissions, cwd) => {
      claudeSpawns.push([workspaceId, skipPermissions, cwd]);
    },
    spawnAgentTab: async (workspaceId, agentId, skipPermissions, cwd, model) => {
      agentSpawns.push([workspaceId, agentId, skipPermissions, cwd, model]);
      return null;
    },
  });

  actions.moveTab("tab-2", "workspace-1", "workspace-2");
  const created = await actions.createTab("workspace-1", "/repo/new");
  actions.closeTab("workspace-1", "tab-2");
  actions.setActiveTab("workspace-1", "tab-2");
  actions.spawnClaudeTab("workspace-1", true, "/repo/claude");
  await actions.spawnAgentTab("workspace-1", "codex", false, "/repo/codex", "gpt-5");

  assert(moves.length === 1, `move called ${moves.length} times`);
  assert(
    moves[0]?.[0] === "tab-2"
      && moves[0]?.[1] === "workspace-1"
      && moves[0]?.[2] === "workspace-2",
    "move receives tabId, fromWorkspaceId, and toWorkspaceId",
  );
  assert(creates.length === 1, `create called ${creates.length} times`);
  assert(
    creates[0]?.[0] === "workspace-1" && creates[0]?.[1] === "/repo/new",
    "create receives workspaceId and cwd",
  );
  assert(created === createdTabResult, "create promise result is honoured");
  assert(closes.length === 1, `close called ${closes.length} times`);
  assert(
    closes[0]?.[0] === "workspace-1" && closes[0]?.[1] === "tab-2",
    "close receives workspaceId and tabId",
  );
  assert(activations.length === 1, `activation called ${activations.length} times`);
  assert(
    activations[0]?.[0] === "workspace-1" && activations[0]?.[1] === "tab-2",
    "activation receives workspaceId and tabId",
  );
  assert(claudeSpawns.length === 1, `Claude spawn called ${claudeSpawns.length} times`);
  assert(
    claudeSpawns[0]?.[0] === "workspace-1"
      && claudeSpawns[0]?.[1] === true
      && claudeSpawns[0]?.[2] === "/repo/claude",
    "Claude spawn receives expected arguments",
  );
  assert(agentSpawns.length === 1, `agent spawn called ${agentSpawns.length} times`);
  assert(
    agentSpawns[0]?.[0] === "workspace-1"
      && agentSpawns[0]?.[1] === "codex"
      && agentSpawns[0]?.[2] === false
      && agentSpawns[0]?.[3] === "/repo/codex"
      && agentSpawns[0]?.[4] === "gpt-5",
    "agent spawn receives expected arguments",
  );
});

test("watcher Tab mounts read-only bar and no composer input", async () => {
  const tabId = "watcher-tab";
  const state = muxStateFor({
    [tabId]: tabBase(tabId, {
      watcher: {
        charter: "Observe health.",
        policy: { tick_interval_secs: 120, nudge_cooldown_secs: 600, max_actions_per_tick: 3 },
      },
    }),
  });
  const { host, root } = await renderTabSessionChat(tabId, state);
  assert(host.querySelector('[data-testid="watcher-readonly-bar"]'), "read-only bar mounted");
  assert(!host.querySelector('[data-testid="chat-composer"]'), "composer absent");
  assert(!host.querySelector('[data-testid="chat-composer-shell"]'), "composer shell absent");
  assert(!host.querySelector("textarea"), "no composer textarea");
  await act(async () => root.unmount());
});

test("normal chat_backend Tab keeps composer and hides read-only bar", async () => {
  const tabId = "chat-tab";
  const state = muxStateFor({
    [tabId]: tabBase(tabId),
  });
  const { host, root } = await renderTabSessionChat(tabId, state);
  const chat = host.querySelector('[data-testid="tab-session-chat"]');
  const messages = host.querySelector('[data-testid="tab-session-chat-messages"]');
  const composer = host.querySelector('[data-testid="chat-composer"]');
  assert(chat?.getAttribute("role") === "region", "chat is an accessible region");
  assert(chat?.getAttribute("aria-label") === "Tab chat: chat-tab", "chat region is named");
  assert(messages?.getAttribute("role") === "log", "conversation is an accessible log");
  assert(messages?.getAttribute("aria-label") === "Conversation", "conversation log is named");
  assert(messages?.classList.contains("select-text"), "conversation text remains selectable");
  assert(composer?.getAttribute("aria-label") === "Message composer", "composer is named");
  assert(composer?.getAttribute("data-tab-id") === tabId, "composer exposes stable Tab scope");
  assert(!host.querySelector('[data-testid="watcher-readonly-bar"]'), "read-only bar absent");
  await act(async () => root.unmount());
});

test("bound-session pending snapshot renders the provider loader instead of false empty", async () => {
  const tabId = "recovering-tab";
  const state = muxStateFor({
    [tabId]: tabBase(tabId, { agent_session_id: "bound-session" }),
  });
  const { host, root } = await renderTabSessionChat(
    tabId,
    state,
    () => ({ text: "", offset: 0, pending: true }),
  );
  assert(host.querySelector('[data-testid="chat-setup-state"]'), "provider loader mounted");
  assert(!host.querySelector('[data-testid="chat-recovering-state"]'), "duplicate recovery loader absent");
  assert(!host.querySelector('[data-testid="chat-empty-state"]'), "false empty state absent");
  assert(host.querySelector('[data-testid="chat-composer"]'), "chat remains interactive");
  await act(async () => root.unmount());
});

test("fresh unbound PTY launch failure remains neutral loading", async () => {
  const tabId = "fresh-unbound-chat";
  const state = muxStateFor({
    [tabId]: tabBase(tabId, {
      backing: { kind: "pty" },
      agent_type: "codex",
      boot_stage: "spawn_failed",
      boot_failure_reason: "late stale failure",
      protected: true,
    }),
  });
  const { host, root } = await renderTabSessionChat(
    tabId,
    state,
    () => ({ text: "", offset: 0, pending: true }),
  );
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
  });
  const chat = host.querySelector('[role="region"][aria-label="Tab chat: fresh-unbound-chat"]');
  const composer = host.querySelector('[aria-label="Message composer"]');
  assert(chat?.getAttribute("aria-busy") === "false", "fresh unbound PTY authority is not announced as recovering");
  assert(!host.querySelector('[data-testid="chat-recovering-state"]'), "fresh chat has no recovery placeholder");
  assert(!host.querySelector('[data-testid="chat-empty-state"]'), "fresh chat has no empty-history placeholder");
  assert(composer, "usable fresh chat keeps its named composer");
  assert(host.querySelector('[data-testid="chat-setup-state"]'), "unbound PTY authority keeps loading");
  assert(!host.querySelector('[role="alert"]'), "no failure alert is rendered");
  await act(async () => root.unmount());
});

test("an attached chat never regresses to a stale launch failure", async () => {
  const tabId = "attached-stale-failure";
  const state = muxStateFor({
    [tabId]: tabBase(tabId, {
      agent_session_id: "bound-session",
      boot_stage: "spawn_failed",
      boot_failure_reason: "stale result",
      protected: true,
    }),
  });
  const { host, root } = await renderTabSessionChat(tabId, state);
  assert(host.querySelector('[data-testid="chat-composer"]'), "attached chat remains usable");
  assert(!host.querySelector('[data-testid="chat-launch-failed"]'), "stale failure cannot cover attached chat");
  await act(async () => root.unmount());
});

test("an attached current session never shows a failure surface", async () => {
  const tabId = "attached-reattached";
  const state = muxStateFor({
    [tabId]: tabBase(tabId, {
      agent_session_id: "current-session",
      boot_stage: "pty_reattached",
      protected: true,
    }),
  });
  const { host, root } = await renderTabSessionChat(tabId, state);
  assert(host.querySelector('[data-testid="chat-composer"]'), "current attached chat remains usable");
  assert(!host.querySelector('[data-testid="chat-launch-failed"]'), "reattach clears any failure presentation");
  await act(async () => root.unmount());
});

test("the durable session rejects stale launch nonces and attaches to current authority", async () => {
  const tabId = "binding-epoch-tab";
  const state = muxStateFor({
    [tabId]: tabBase(tabId, {
      backing: { kind: "pty" },
      agent_session_id: "bound-session",
      session_binding_nonce: "current-epoch",
      boot_stage: "spawn_failed",
      boot_failure_reason: "late stale failure",
      protected: true,
    }),
  });
  const { host, root, emitChat } = await renderTabSessionChat(
    tabId,
    state,
    () => ({ text: "", offset: 0, pending: true }),
  );
  const composer = host.querySelector('[data-testid="chat-composer"]');
  assert(host.querySelector('[data-testid="chat-setup-state"]'), "pre-attach state remains neutral loading");

  await act(async () => {
    emitChat({
      tab_id: tabId,
      source_session_id: "bound-session",
      session_binding_nonce: "previous-epoch",
      messages: [{ id: "same-session-event", type: "assistant", content: "attached", timestamp: "1" }],
    });
  });
  assert(host.querySelector('[data-testid="chat-setup-state"]'), "stale launch nonce cannot claim current authority");
  assert(!host.textContent?.includes("attached"), "stale launch nonce cannot alter the transcript");

  await act(async () => {
    emitChat({
      tab_id: tabId,
      source_session_id: "bound-session",
      session_binding_nonce: "current-epoch",
      messages: [{ id: "current-session-event", type: "assistant", content: "attached", timestamp: "2" }],
    });
  });
  assert(!host.querySelector('[data-testid="chat-launch-failed"]'), "current authority never exposes a failure surface");
  assert(host.textContent?.includes("attached"), "current-authority event updates the transcript");

  await act(async () => {
    emitChat({
      tab_id: tabId,
      source_session_id: "foreign-session",
      session_binding_nonce: "current-epoch",
      messages: [{ id: "foreign-event", type: "assistant", content: "foreign", timestamp: "3" }],
    });
  });
  assert(host.querySelector('[data-testid="chat-composer"]') === composer, "attachment keeps the composer mounted");
  assert(!host.textContent?.includes("foreign"), "a different session cannot alter the transcript");
  await act(async () => root.unmount());
});

test("launch failure has no inline failure or retry state", async () => {
  const tabId = "generic-retry-tab";
  const state = muxStateFor({
    [tabId]: tabBase(tabId, {
      agent_session_id: "bound-session",
      boot_stage: "spawn_failed",
      boot_failure_reason: "launch failed",
      protected: true,
    }),
  });
  const { host, root, commands } = await renderTabSessionChat(
    tabId,
    state,
    () => ({ text: "", offset: 0, pending: true }),
  );
  assert(host.querySelector('[data-testid="chat-setup-state"]'), "neutral loader remains visible");
  assert(!host.querySelector('[data-testid="chat-launch-retry"]'), "inline retry is absent");
  assert(!host.querySelector('[data-testid="chat-launch-failed"]'), "failure card is absent");
  assert(!commands.includes("restart_agent_tab"), "loading UI does not mutate agent lifecycle");
  await act(async () => root.unmount());
});

test("T7 awaiting-store keeps the composer identity, draft, and focus", async () => {
  const tabId = "pending-composer-tab";
  const state = muxStateFor({
    [tabId]: tabBase(tabId, { agent_session_id: "bound-session" }),
  });
  const { host, root, commands } = await renderTabSessionChat(
    tabId,
    state,
    () => ({ text: "", offset: 0, pending: true }),
  );
  const composer = host.querySelector('[data-testid="chat-composer"]') as HTMLTextAreaElement | null;
  assert(composer, "empty chat keeps its composer available");
  const setter = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")?.set;
  setter?.call(composer, "typing without reset");
  composer.dispatchEvent(new Event("input", { bubbles: true }));
  composer.setSelectionRange(7, 7);
  composer.focus();

  await act(async () => new Promise((resolve) => setTimeout(resolve, 1_100)));

  const currentComposer = host.querySelector('[data-testid="chat-composer"]') as HTMLTextAreaElement | null;
  assert(currentComposer === composer, "pending empty chat did not remount the composer");
  assert(currentComposer.value === "typing without reset", "typed draft survived pending chat state");
  assert(currentComposer.selectionStart === 7, "caret position survived pending chat state");
  assert(document.activeElement === composer, "composer focus survived pending chat state");
  assert(commands.filter((command) => command === "chat_start").length === 1, "awaiting-store chat does not poll or remount");
  await act(async () => root.unmount());
});

test("snapshot failure stays on the neutral provider loader", async () => {
  const tabId = "failed-tab";
  const state = muxStateFor({
    [tabId]: tabBase(tabId, { agent_session_id: "bound-session" }),
  });
  const { host, root } = await renderTabSessionChat(tabId, state, () => {
    throw new Error("snapshot unavailable");
  });
  assert(host.querySelector('[data-testid="chat-setup-state"]'), "provider loader remains mounted");
  assert(!host.querySelector('[data-testid="chat-recovery-error"]'), "error state is not rendered");
  assert(!host.querySelector('[data-testid="chat-retry"]'), "inline retry is not rendered");
  assert(!host.querySelector('[data-testid="chat-empty-state"]'), "failure is not shown as empty");
  await act(async () => root.unmount());
});

test("mount failure remains loading without releasing a shared tailer", async () => {
  const tabId = "reattach-tab";
  const state = muxStateFor({
    [tabId]: tabBase(tabId, { agent_session_id: "bound-session" }),
  });
  let starts = 0;
  const { host, root, commands } = await renderTabSessionChat(tabId, state, () => {
    starts += 1;
    if (starts === 1) throw new Error("snapshot unavailable");
    return {
      text: "",
      offset: 42,
      messages: [{ id: "history-1", type: "assistant", content: "restored history" }],
    };
  });
  assert(host.querySelector('[data-testid="chat-setup-state"]'), "mount failure remains neutral loading");
  assert(!host.querySelector('[data-testid="chat-retry"]'), "no inline reconnect surface remains");
  assert(commands.filter((command) => command === "chat_stop").length === 0, "manual reconnect never releases another view lease");
  assert(commands.filter((command) => command === "chat_start").length === 1, "mount performs one attach attempt");
  await act(async () => root.unmount());
});

test("Tab header reconnect control requests a hook-owned reattach", async () => {
  const { createRequire } = await import("node:module");
  const require = createRequire(__filename);
  require.extensions[".css"] = () => {};
  const { __setInvokeForTest } = await import("../../lib/bridge");
  const { TabTopBar } = await import("./TabTopBar");
  const commands: string[] = [];
  let reattachEvents = 0;
  window.addEventListener("codeterm:chat-reattached", () => {
    reattachEvents += 1;
  });
  __setInvokeForTest((async (cmd: string) => {
    commands.push(cmd);
    return {};
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
  }) as any);

  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(
      <TabTopBar
        tabId="header-tab"
        cwd={null}
        showSwitch={false}
        canReconnect={true}
        isActive={true}
      />,
    );
  });
  const trigger = host.querySelector('[data-testid="tab-actions-trigger"]') as HTMLButtonElement | null;
  assert(trigger, "compact tab actions control is visible");
  await act(async () => trigger.click());
  const reconnect = document.querySelector('[data-testid="chat-reconnect"]') as HTMLButtonElement | null;
  assert(reconnect, "reconnect is available inside tab actions");
  await act(async () => reconnect.click());
  assert(!commands.includes("chat_stop"), "reconnect leaves shared tailer ownership to mounted views");
  assert(reattachEvents === 1, "reconnect asks the mounted chat hook to attach once");
  const shortcut = new window.KeyboardEvent("keydown", {
    code: "KeyR",
    key: "r",
    ctrlKey: true,
    bubbles: true,
    cancelable: true,
  });
  await act(async () => window.dispatchEvent(shortcut));
  assert(shortcut.defaultPrevented, "Ctrl+R prevents a WebView reload");
  assert(!commands.includes("chat_stop"), "Ctrl+R reattaches without releasing shared tailer ownership");
  await act(async () => root.unmount());
  host.remove();
});

test("Tab actions restart the agent session and copy its ID", async () => {
  const { __setInvokeForTest } = await import("../../lib/bridge");
  const { TabTopBar } = await import("./TabTopBar");
  const commands: string[] = [];
  let copied = "";
  __setInvokeForTest((async (cmd: string) => {
    commands.push(cmd);
    return {};
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
  }) as any);
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: { writeText: async (value: string) => { copied = value; } },
  });

  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(
      <TabTopBar
        tabId="agent-tab"
        cwd={null}
        showSwitch={false}
        canReconnect={true}
        canRestartSession={true}
        sessionId="session-123"
      />,
    );
  });

  const trigger = host.querySelector('[data-testid="tab-actions-trigger"]') as HTMLButtonElement;
  assert(trigger.getAttribute("aria-haspopup") === "menu", "tab actions expose menu semantics");
  await act(async () => trigger.click());
  const menu = document.querySelector('[data-testid="tab-actions-menu"]');
  assert(menu?.getAttribute("role") === "menu", "tab actions render an accessible menu");
  assert(menu.textContent?.includes("session-123"), "menu identifies the current session");
  const copy = menu.querySelector('[data-testid="tab-copy-session-id"]') as HTMLButtonElement;
  assert(copy.getAttribute("aria-label") === "Copy session ID", "session ID has an accessible adjacent copy icon");
  assert(copy.textContent?.trim() === "", "copy control is icon-only beside the session ID");
  await act(async () => copy.click());
  assert(copied === "session-123", "copy action writes the current session ID");

  await act(async () => trigger.click());
  const restart = document.querySelector('[data-testid="tab-restart-session"]') as HTMLButtonElement;
  await act(async () => restart.click());
  assert(commands.includes("restart_agent_tab"), "restart action uses the generic session-preserving restart");

  await act(async () => root.unmount());
  host.remove();
});

test("Tab header does not repeat the mobile top safe area", async () => {
  const { TabTopBar } = await import("./TabTopBar");
  document.documentElement.style.setProperty("--ct-safe-top", "44px");
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(
      <TabTopBar
        tabId="mobile-header-tab"
        cwd={null}
        showSwitch={false}
      />,
    );
  });
  const header = host.querySelector('[data-testid="tab-top-bar"]') as HTMLDivElement | null;
  assert(header?.style.paddingTop === "6px", "Tab header keeps only its local 6px padding");
  assert(!header.style.paddingTop.includes("safe-top"), "titlebar remains the sole top safe-area owner");
  await act(async () => root.unmount());
  host.remove();
  document.documentElement.style.removeProperty("--ct-safe-top");
});

async function main() {
  let failed = 0;
  for (const [name, fn] of tests) {
    try {
      await fn();
      console.log(`PASS ${name}`);
    } catch (error) {
      failed += 1;
      console.error(`FAIL ${name}`, error);
    }
  }
  if (failed > 0) throw new Error(`${failed} test(s) failed`);
  const { __setInvokeForTest, __setListenForTest } = await import("../../lib/bridge");
  __setInvokeForTest(null);
  __setListenForTest(null);
  await testWindow.happyDOM.abort();
  testWindow.close();
}

main().then(() => process.exit(0)).catch((error) => {
  console.error(error);
  process.exit(1);
});
