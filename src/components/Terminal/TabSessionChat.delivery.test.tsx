import { act } from "react";
import { Window } from "happy-dom";
import { createRoot, type Root } from "react-dom/client";
import type { Tab } from "../../lib/types";
import {
  MultiplexerActionsContext,
  MuxStateStoreContext,
  type MultiplexerActionsContextType,
} from "../../store/multiplexer/context";
import { createMuxStateStore } from "../../store/multiplexer/selectorStore";
import type { ExtendedState } from "../../store/multiplexer/tree";
import {
  resetChatSendStatusForTest,
  trackChatSend,
  type ChatSendStatus,
} from "./chatSendStatus";

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
  Object.defineProperty(globalThis, "navigator", { value: window.navigator, configurable: true });
  Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
  Object.defineProperty(globalThis, "HTMLButtonElement", { value: window.HTMLButtonElement, configurable: true });
  Object.defineProperty(globalThis, "HTMLTextAreaElement", { value: window.HTMLTextAreaElement, configurable: true });
  Object.defineProperty(globalThis, "SVGElement", { value: window.SVGElement, configurable: true });
  Object.defineProperty(globalThis, "Element", { value: window.Element, configurable: true });
  Object.defineProperty(globalThis, "Node", { value: window.Node, configurable: true });
  Object.defineProperty(globalThis, "Event", { value: window.Event, configurable: true });
  Object.defineProperty(globalThis, "CustomEvent", { value: window.CustomEvent, configurable: true });
  Object.defineProperty(globalThis, "KeyboardEvent", { value: window.KeyboardEvent, configurable: true });
  Object.defineProperty(globalThis, "InputEvent", { value: window.InputEvent, configurable: true });
  Object.defineProperty(globalThis, "requestAnimationFrame", {
    configurable: true,
    value: (callback: FrameRequestCallback) => {
      callback(0);
      return 0;
    },
  });
  Object.defineProperty(globalThis, "cancelAnimationFrame", { configurable: true, value: () => {} });
  class StubResizeObserver {
    observe(): void {}
    unobserve(): void {}
    disconnect(): void {}
  }
  Object.defineProperty(globalThis, "ResizeObserver", { configurable: true, value: StubResizeObserver });
  return window;
}

function implicitRole(element: Element): string | null {
  const explicit = element.getAttribute("role");
  if (explicit) return explicit;
  if (element.tagName === "BUTTON") return "button";
  if (element.tagName === "TEXTAREA") return "textbox";
  return null;
}

function accessibleName(element: Element): string {
  return element.getAttribute("aria-label")?.trim()
    || element.textContent?.trim()
    || element.getAttribute("title")?.trim()
    || "";
}

function getByRole<T extends Element>(
  root: ParentNode,
  role: string,
  name: RegExp,
): T {
  const match = Array.from(root.querySelectorAll("*")).find(
    (element) => implicitRole(element) === role && name.test(accessibleName(element)),
  );
  if (!match) throw new Error(`No ${role} with accessible name ${name}`);
  return match as T;
}

function tabState(tabId: string): ExtendedState {
  const tab = {
    id: tabId,
    pty_id: null,
    tab_type: "Terminal",
    title: "Delivery test",
    cwd: null,
    backing: { kind: "chat_backend", plugin_id: "lmstudio", session_id: "session-1" },
  } as Tab;
  return {
    workspaces: [],
    tabs: { [tabId]: tab },
    teams: [],
    active_workspace_id: null,
    focused_tab_id: tabId,
    notifications: [],
    tabStatuses: {},
    toasts: [],
    activeTabIds: {},
    generation: 1,
    pendingCloses: [],
  };
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
  };
}

function status(
  sendId: string,
  state: ChatSendStatus["state"],
  sequence: number,
  error: string | null = null,
): ChatSendStatus {
  return {
    send_id: sendId,
    tab_id: "delivery-tab",
    state,
    error,
    ledger_id: "ledger-1",
    sequence,
    retained_from: 1,
  };
}

async function renderChat(
  sendResults: ChatSendStatus[],
  commands: string[],
): Promise<{
  host: HTMLDivElement;
  root: Root;
  emitStatus: (next: ChatSendStatus) => void;
}> {
  const { __setInvokeForTest, __setListenForTest } = await import("../../lib/bridge");
  const { TabSessionChat } = await import("./TabSessionChat");
  let statusListener: ((event: { payload: ChatSendStatus }) => void) | null = null;

  __setInvokeForTest((async (command: string) => {
    commands.push(command);
    if (command === "chat_start") {
      return {
        text: "",
        offset: 0,
        messages: [],
        source_session_id: null,
        session_binding_nonce: null,
        subscription_id: "delivery-subscription",
      };
    }
    if (command === "chat_send_status" || command === "delayed_send_list") return [];
    if (command === "chat_send_text") {
      const next = sendResults.shift();
      if (!next) throw new Error("unexpected ledger send");
      return next;
    }
    return {};
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
  }) as any);
  __setListenForTest(async (event, handler) => {
    if (event === "chat-send-status") {
      statusListener = handler as (event: { payload: ChatSendStatus }) => void;
    }
    return () => {
      if (event === "chat-send-status") statusListener = null;
    };
  });

  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  const store = createMuxStateStore(() => tabState("delivery-tab"));
  await act(async () => {
    root.render(
      <MuxStateStoreContext.Provider value={store}>
        <MultiplexerActionsContext.Provider value={muxActions()}>
          <TabSessionChat tabId="delivery-tab" chatBackend isVisible />
        </MultiplexerActionsContext.Provider>
      </MuxStateStoreContext.Provider>,
    );
  });
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
  });
  return {
    host,
    root,
    emitStatus: (next) => statusListener?.({ payload: next }),
  };
}

async function trackPending(pending: ChatSendStatus, message: string): Promise<void> {
  await act(async () => {
    trackChatSend(pending, {
      message_id: `opt-${pending.send_id}`,
      message,
    });
    await Promise.resolve();
  });
}

test("submit_unconfirmed is attached to its message and retry uses the ledger", async () => {
  resetChatSendStatusForTest();
  const commands: string[] = [];
  const pending = status("send-1", "pending", 1);
  const retryPending = status("send-2", "pending", 2);
  const { host, root, emitStatus } = await renderChat([retryPending], commands);
  const warn = console.warn;
  let warnCalls = 0;
  console.warn = () => {
    warnCalls += 1;
  };
  try {
    await trackPending(pending, "Ship the visible ledger");
    const optimistic = getByRole<HTMLElement>(host, "article", /ship the visible ledger/i);
    getByRole(optimistic, "status", /sending/i);

    await act(async () => {
      emitStatus(status("send-1", "failed", 1, "submit_unconfirmed: submit key was not acknowledged"));
    });
    const failedMessage = getByRole<HTMLElement>(host, "article", /ship the visible ledger/i);
    const alert = getByRole<HTMLElement>(failedMessage, "alert", /submission unconfirmed/i);
    const retry = getByRole<HTMLButtonElement>(alert, "button", /retry/i);
    assert(retry.disabled === false, "retry is enabled");
    assert(retry.tabIndex === 0, "retry participates in keyboard focus order");
    assert(warnCalls === 0, "delivery failure does not terminate in console.warn");

    await act(async () => {
      retry.click();
      retry.click();
      await Promise.resolve();
    });
    assert(
      commands.filter((command) => command === "chat_send_text").length === 1,
      "retry re-enters the chat-send ledger once while recovery is pending",
    );
    assert(!commands.includes("write_pty_string"), "retry never writes the payload directly to a PTY");
  } finally {
    console.warn = warn;
    await act(async () => root.unmount());
    host.remove();
  }
});

test("terminal sent state reconciles the optimistic message", async () => {
  resetChatSendStatusForTest();
  const commands: string[] = [];
  const pending = status("send-3", "pending", 3);
  const { host, root, emitStatus } = await renderChat([], commands);
  await trackPending(pending, "Reconcile this message");
  const optimistic = getByRole<HTMLElement>(host, "article", /reconcile this message/i);
  getByRole(optimistic, "status", /sending/i);

  await act(async () => {
    emitStatus(status("send-3", "sent", 3));
  });
  const reconciled = getByRole<HTMLElement>(host, "article", /reconcile this message/i);
  getByRole(reconciled, "status", /submission confirmed/i);

  await act(async () => root.unmount());
  host.remove();
});

test("failed message and retry remain visible after remount", async () => {
  resetChatSendStatusForTest();
  const firstCommands: string[] = [];
  const pending = status("send-4", "pending", 4);
  const first = await renderChat([], firstCommands);
  await trackPending(pending, "Persist this failed message");
  await act(async () => {
    first.emitStatus(status("send-4", "failed", 4, "submit_unconfirmed"));
  });
  await act(async () => first.root.unmount());
  first.host.remove();

  const remount = await renderChat([status("send-5", "pending", 5)], []);
  const message = getByRole<HTMLElement>(remount.host, "article", /persist this failed message/i);
  const alert = getByRole<HTMLElement>(message, "alert", /submission unconfirmed/i);
  getByRole(alert, "button", /retry/i);

  await act(async () => remount.root.unmount());
  remount.host.remove();
});

async function main() {
  const window = setupDom();
  try {
    for (const [name, fn] of tests) {
      try {
        await fn();
        console.log(`✓ ${name}`);
      } catch (error) {
        console.error(`✗ ${name}`);
        throw error;
      }
    }
  } finally {
    await window.happyDOM.abort();
  }
}

void main().then(
  () => process.exit(0),
  (error) => {
    console.error(error);
    process.exit(1);
  },
);
