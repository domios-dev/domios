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
    openSessionPicker: noop,
    closeSessionPicker: noop,
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
    virtualTabs: {},
  };
}

async function renderPaneSessionChat(
  paneId: string,
  state: ExtendedState,
  chatStart: () => unknown = () => ({ text: "", offset: 0, messages: [] }),
): Promise<{ host: HTMLDivElement; root: Root; commands: string[] }> {
  const { __setInvokeForTest, __setListenForTest } = await import("../../lib/bridge");
  const { PaneSessionChat } = await import("./PaneSessionChat");

  const commands: string[] = [];
  __setInvokeForTest((async (cmd: string) => {
    commands.push(cmd);
    if (cmd === "chat_start") return chatStart();
    if (cmd === "delayed_send_list") return [];
    return {};
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
  }) as any);
  __setListenForTest(async () => () => {});

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
  return { host, root, commands };
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
  assert(host.querySelector('[data-testid="chat-composer"]'), "composer present");
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

test("T6 manual reconnect performs one stop/start pair, clears the error, and restores history", async () => {
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
  assert(commands.filter((command) => command === "chat_stop").length === 1, "manual reconnect stops the stale tailer once");
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
        showChatReconnect={true}
      />,
    );
  });
  const reconnect = host.querySelector('[data-testid="chat-reconnect"]') as HTMLButtonElement | null;
  assert(reconnect, "header reconnect control is visible");
  await act(async () => reconnect.click());
  const stopAt = commands.indexOf("chat_stop");
  assert(stopAt >= 0, "reconnect calls chat_stop");
  assert(reattachEvents === 1, "reconnect asks the mounted chat hook to attach once");
  await act(async () => root.unmount());
  host.remove();
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
