// Run: npx tsx src/components/Terminal/PaneSessionChat.watcher.test.tsx

import { act } from "react";
import { Window } from "happy-dom";
import { createRoot, type Root } from "react-dom/client";
import type { Pane } from "../../lib/types";
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
}

function paneBase(id: string, overrides: Partial<Pane> = {}): Pane {
  return {
    id,
    pty_id: null,
    pane_type: "Terminal",
    title: id,
    cwd: null,
    backing: { kind: "chat_backend", plugin_id: "lmstudio", session_id: "sess-1" },
    ...overrides,
  } as Pane;
}

function muxActions(): MultiplexerActionsContextType {
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
    selectTabByPane: noopAsync,
    closePane: noop,
    setFocusedPane: noop,
    setPanePty: noop,
    addNotification: noop,
    upsertToast: noop,
    markRead: noop,
    markAllRead: noop,
    deleteNotification: noop,
    clearNotifications: noop,
    dismissToast: noop,
    setPaneTitle: noop,
    setTabTitle: noop,
    setAgentType: noop,
    updatePaneStatus: noop,
    spawnClaudeTab: noop,
    spawnAgentTab: async () => null,
    createOrchestrationPair: async () => {
      throw new Error("not implemented");
    },
  };
}

function muxStateFor(panes: Record<string, Pane>): ExtendedState {
  return {
    workspaces: [],
    panes,
    teams: [],
    active_workspace_id: null,
    focused_pane_id: Object.keys(panes)[0] ?? null,
    notifications: [],
    paneStatuses: {},
    toasts: [],
    activeTabIds: {},
    generation: 1,
    pendingCloses: [],
  };
}

async function renderPaneSessionChat(
  paneId: string,
  state: ExtendedState,
  chatStart: () => unknown = () => ({ text: "", offset: 0, messages: [] }),
): Promise<{
  host: HTMLDivElement;
  root: Root;
  commands: string[];
  emitChat: (payload: { pane_id: string; messages: Array<{ id: string; type: "assistant"; content: string; timestamp: string }>; source_session_id: string | null; session_binding_nonce: string | null }) => void;
}> {
  const { __setInvokeForTest, __setListenForTest } = await import("../../lib/bridge");
  const { PaneSessionChat } = await import("./PaneSessionChat");

  const commands: string[] = [];
  let chatListener: ((event: { payload: unknown }) => void) | null = null;
  __setInvokeForTest((async (cmd: string) => {
    commands.push(cmd);
    if (cmd === "chat_start") {
      return {
        source_session_id: state.panes[paneId]?.agent_session_id ?? null,
        session_binding_nonce: state.panes[paneId]?.session_binding_nonce ?? null,
        subscription_id: "test-subscription",
        ...(chatStart() as object),
      };
    }
    if (cmd === "delayed_send_list") return [];
    return {};
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
  }) as any);
  __setListenForTest(async (event, handler) => {
    if (event === `pane-chat-msg-${paneId}`) chatListener = handler as (event: { payload: unknown }) => void;
    return () => {
      if (event === `pane-chat-msg-${paneId}`) chatListener = null;
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
          <PaneSessionChat paneId={paneId} chatBackend isVisible />
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

setupDom();

test("watcher pane mounts read-only bar and no composer input", async () => {
  const paneId = "watcher-pane";
  const state = muxStateFor({
    [paneId]: paneBase(paneId, {
      watcher: {
        charter: "Observe health.",
        policy: { tick_interval_secs: 120, nudge_cooldown_secs: 600, max_actions_per_tick: 3 },
      },
    }),
  });
  const { host, root } = await renderPaneSessionChat(paneId, state);
  assert(host.querySelector('[data-testid="watcher-readonly-bar"]'), "read-only bar mounted");
  assert(!host.querySelector('[data-testid="chat-composer"]'), "composer absent");
  assert(!host.querySelector('[data-testid="chat-composer-shell"]'), "composer shell absent");
  assert(!host.querySelector("textarea"), "no composer textarea");
  await act(async () => root.unmount());
});

test("normal chat_backend pane keeps composer and hides read-only bar", async () => {
  const paneId = "chat-pane";
  const state = muxStateFor({
    [paneId]: paneBase(paneId),
  });
  const { host, root } = await renderPaneSessionChat(paneId, state);
  const chat = host.querySelector('[data-testid="pane-session-chat"]');
  const messages = host.querySelector('[data-testid="pane-session-chat-messages"]');
  const composer = host.querySelector('[data-testid="chat-composer"]');
  assert(chat?.getAttribute("role") === "region", "chat is an accessible region");
  assert(chat?.getAttribute("aria-label") === "Tab chat: chat-pane", "chat region is named");
  assert(messages?.getAttribute("role") === "log", "conversation is an accessible log");
  assert(messages?.getAttribute("aria-label") === "Conversation", "conversation log is named");
  assert(composer?.getAttribute("aria-label") === "Message composer", "composer is named");
  assert(composer?.getAttribute("data-pane-id") === paneId, "composer exposes stable pane scope");
  assert(!host.querySelector('[data-testid="watcher-readonly-bar"]'), "read-only bar absent");
  await act(async () => root.unmount());
});

test("bound-session pending snapshot renders recovery instead of false empty", async () => {
  const paneId = "recovering-pane";
  const state = muxStateFor({
    [paneId]: paneBase(paneId, { agent_session_id: "bound-session" }),
  });
  const { host, root } = await renderPaneSessionChat(
    paneId,
    state,
    () => ({ text: "", offset: 0, pending: true }),
  );
  assert(host.querySelector('[data-testid="chat-recovering-state"]'), "recovery state mounted");
  assert(!host.querySelector('[data-testid="chat-empty-state"]'), "false empty state absent");
  assert(host.querySelector('[data-testid="chat-composer"]'), "chat remains interactive");
  await act(async () => root.unmount());
});

test("fresh unbound chat keeps its usable surface while the transcript store is pending", async () => {
  const paneId = "fresh-unbound-chat";
  const state = muxStateFor({
    [paneId]: paneBase(paneId, {
      backing: { kind: "pty" },
      agent_type: "codex",
      boot_stage: "spawn_failed",
      boot_failure_reason: "late stale failure",
      protected: true,
    }),
  });
  const { host, root } = await renderPaneSessionChat(
    paneId,
    state,
    () => ({ text: "", offset: 0, pending: true }),
  );
  const chat = host.querySelector('[role="region"][aria-label="Tab chat: fresh-unbound-chat"]');
  const composer = host.querySelector('[aria-label="Message composer"]');
  assert(chat?.getAttribute("aria-busy") === "false", "usable fresh chat is not announced as loading");
  assert(composer, "usable fresh chat keeps its named composer");
  assert(!host.querySelector('[role="alert"][aria-label="Launch failed"]'), "late failure cannot cover a usable fresh chat");
  await act(async () => root.unmount());
});

test("an attached chat never regresses to a stale launch failure", async () => {
  const paneId = "attached-stale-failure";
  const state = muxStateFor({
    [paneId]: paneBase(paneId, {
      agent_session_id: "bound-session",
      boot_stage: "spawn_failed",
      boot_failure_reason: "stale result",
      protected: true,
    }),
  });
  const { host, root } = await renderPaneSessionChat(paneId, state);
  assert(host.querySelector('[data-testid="chat-composer"]'), "attached chat remains usable");
  assert(!host.querySelector('[data-testid="chat-launch-failed"]'), "stale failure cannot cover attached chat");
  await act(async () => root.unmount());
});

test("an attached current session ignores a late PTY reattach stage", async () => {
  const paneId = "attached-reattached";
  const state = muxStateFor({
    [paneId]: paneBase(paneId, {
      agent_session_id: "current-session",
      boot_stage: "pty_reattached",
      protected: true,
    }),
  });
  const { host, root } = await renderPaneSessionChat(paneId, state);
  assert(host.querySelector('[data-testid="chat-composer"]'), "current attached chat remains usable");
  assert(!host.querySelector('[data-testid="chat-setup-state"]'), "late PTY reattach cannot cover current chat");
  assert(!host.querySelector('[data-testid="chat-launch-failed"]'), "reattach clears any failure presentation");
  await act(async () => root.unmount());
});

test("the durable session attaches across a launch nonce rotation", async () => {
  const paneId = "binding-epoch-pane";
  const state = muxStateFor({
    [paneId]: paneBase(paneId, {
      backing: { kind: "pty" },
      agent_session_id: "bound-session",
      session_binding_nonce: "current-epoch",
      boot_stage: "spawn_failed",
      boot_failure_reason: "late stale failure",
      protected: true,
    }),
  });
  const { host, root, emitChat } = await renderPaneSessionChat(
    paneId,
    state,
    () => ({ text: "", offset: 0, pending: true }),
  );
  const composer = host.querySelector('[data-testid="chat-composer"]');
  assert(host.querySelector('[data-testid="chat-launch-failed"]'), "pre-attach failure remains visible");

  await act(async () => {
    emitChat({
      pane_id: paneId,
      source_session_id: "bound-session",
      session_binding_nonce: "previous-epoch",
      messages: [{ id: "same-session-event", type: "assistant", content: "attached", timestamp: "1" }],
    });
  });
  assert(!host.querySelector('[data-testid="chat-launch-failed"]'), "launch nonce rotation cannot invalidate the durable transcript");
  assert(host.textContent?.includes("attached"), "same-session event updates the transcript");

  await act(async () => {
    emitChat({
      pane_id: paneId,
      source_session_id: "foreign-session",
      session_binding_nonce: "current-epoch",
      messages: [{ id: "foreign-event", type: "assistant", content: "foreign", timestamp: "2" }],
    });
  });
  assert(host.querySelector('[data-testid="chat-composer"]') === composer, "attachment keeps the composer mounted");
  assert(!host.textContent?.includes("foreign"), "a different session cannot alter the transcript");
  await act(async () => root.unmount());
});

test("launch retry uses the generic pane restart command", async () => {
  const paneId = "generic-retry-pane";
  const state = muxStateFor({
    [paneId]: paneBase(paneId, {
      agent_session_id: "bound-session",
      boot_stage: "spawn_failed",
      boot_failure_reason: "launch failed",
      protected: true,
    }),
  });
  const { host, root, commands } = await renderPaneSessionChat(
    paneId,
    state,
    () => ({ text: "", offset: 0, pending: true }),
  );
  const retry = host.querySelector('[data-testid="chat-launch-retry"]') as HTMLButtonElement | null;
  assert(retry, "unattached failed launch exposes retry");
  await act(async () => retry.click());
  assert(commands.includes("restart_agent_pane"), "retry targets the current pane through the generic restart seam");
  assert(!commands.includes("restart_general_agent_pane"), "retry never assumes this pane is the General Agent");
  await act(async () => root.unmount());
});

test("T7 awaiting-store keeps the composer identity, draft, and focus", async () => {
  const paneId = "pending-composer-pane";
  const state = muxStateFor({
    [paneId]: paneBase(paneId, { agent_session_id: "bound-session" }),
  });
  const { host, root, commands } = await renderPaneSessionChat(
    paneId,
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

test("snapshot failure keeps a retryable chat surface", async () => {
  const paneId = "failed-pane";
  const state = muxStateFor({
    [paneId]: paneBase(paneId, { agent_session_id: "bound-session" }),
  });
  const { host, root } = await renderPaneSessionChat(paneId, state, () => {
    throw new Error("snapshot unavailable");
  });
  assert(host.querySelector('[data-testid="chat-recovery-error"]'), "error state mounted");
  assert(host.querySelector('[data-testid="chat-retry"]'), "retry remains reachable");
  assert(!host.querySelector('[data-testid="chat-empty-state"]'), "failure is not shown as empty");
  await act(async () => root.unmount());
});

test("T6 manual reconnect resubscribes without releasing a shared tailer", async () => {
  const paneId = "reattach-pane";
  const state = muxStateFor({
    [paneId]: paneBase(paneId, { agent_session_id: "bound-session" }),
  });
  let starts = 0;
  const { host, root, commands } = await renderPaneSessionChat(paneId, state, () => {
    starts += 1;
    if (starts === 1) throw new Error("snapshot unavailable");
    return {
      text: "",
      offset: 42,
      messages: [{ id: "history-1", type: "assistant", content: "restored history" }],
    };
  });
  const reconnect = host.querySelector('[data-testid="chat-retry"]') as HTMLButtonElement | null;
  assert(reconnect, "reconnect remains reachable after mount failure");
  await act(async () => reconnect.click());
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
  });
  assert(commands.filter((command) => command === "chat_stop").length === 0, "manual reconnect never releases another view lease");
  assert(commands.filter((command) => command === "chat_start").length === 2, "manual reconnect starts once after the failed attach");
  assert(!host.querySelector('[data-testid="chat-recovery-error"]'), "manual reconnect clears the error");
  assert(host.textContent?.includes("restored history"), "history reappears after reconnect");
  await act(async () => root.unmount());
});

test("pane header reconnect control requests a hook-owned reattach", async () => {
  const { createRequire } = await import("node:module");
  const require = createRequire(__filename);
  require.extensions[".css"] = () => {};
  const { __setInvokeForTest } = await import("../../lib/bridge");
  const { PaneTopBar } = await import("./PaneTopBar");
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
      <PaneTopBar
        paneId="header-pane"
        cwd={null}
        showSwitch={false}
        chatFirst={false}
        chatDefaultPrompted={true}
        canReconnect={true}
        isActive={true}
      />,
    );
  });
  const trigger = host.querySelector('[data-testid="pane-actions-trigger"]') as HTMLButtonElement | null;
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

test("pane actions restart the agent session and copy its ID", async () => {
  const { __setInvokeForTest } = await import("../../lib/bridge");
  const { PaneTopBar } = await import("./PaneTopBar");
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
      <PaneTopBar
        paneId="agent-pane"
        cwd={null}
        showSwitch={false}
        chatFirst={false}
        chatDefaultPrompted={true}
        canReconnect={true}
        canRestartSession={true}
        sessionId="session-123"
      />,
    );
  });

  const trigger = host.querySelector('[data-testid="pane-actions-trigger"]') as HTMLButtonElement;
  assert(trigger.getAttribute("aria-haspopup") === "menu", "tab actions expose menu semantics");
  await act(async () => trigger.click());
  const menu = document.querySelector('[data-testid="pane-actions-menu"]');
  assert(menu?.getAttribute("role") === "menu", "tab actions render an accessible menu");
  assert(menu.textContent?.includes("session-123"), "menu identifies the current session");
  const copy = menu.querySelector('[data-testid="pane-copy-session-id"]') as HTMLButtonElement;
  assert(copy.getAttribute("aria-label") === "Copy session ID", "session ID has an accessible adjacent copy icon");
  assert(copy.textContent?.trim() === "", "copy control is icon-only beside the session ID");
  await act(async () => copy.click());
  assert(copied === "session-123", "copy action writes the current session ID");

  await act(async () => trigger.click());
  const restart = document.querySelector('[data-testid="pane-restart-session"]') as HTMLButtonElement;
  await act(async () => restart.click());
  assert(commands.includes("restart_agent_pane"), "restart action uses the generic session-preserving restart");

  await act(async () => root.unmount());
  host.remove();
});

test("pane header does not repeat the mobile top safe area", async () => {
  const { PaneTopBar } = await import("./PaneTopBar");
  document.documentElement.style.setProperty("--ct-safe-top", "44px");
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(
      <PaneTopBar
        paneId="mobile-header-pane"
        cwd={null}
        showSwitch={false}
        chatFirst={false}
        chatDefaultPrompted={true}
      />,
    );
  });
  const header = host.querySelector('[data-testid="pane-top-bar"]') as HTMLDivElement | null;
  assert(header?.style.paddingTop === "6px", "pane header keeps only its local 6px padding");
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
}

void main();
