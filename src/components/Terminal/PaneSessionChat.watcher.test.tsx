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

async function renderPaneSessionChat(paneId: string, state: ExtendedState): Promise<{ host: HTMLDivElement; root: Root }> {
  const { __setInvokeForTest, __setListenForTest } = await import("../../lib/bridge");
  const { PaneSessionChat } = await import("./PaneSessionChat");

  __setInvokeForTest((async (cmd: string) => {
    if (cmd === "chat_start") return { text: "", offset: 0, messages: [] };
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
  return { host, root };
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
