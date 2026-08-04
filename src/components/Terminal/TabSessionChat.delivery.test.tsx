import { act } from "react";
import { existsSync, readFileSync } from "node:fs";
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

async function renderChat(): Promise<{ host: HTMLDivElement; root: Root }> {
  const { __setInvokeForTest, __setListenForTest } = await import("../../lib/bridge");
  const { TabSessionChat } = await import("./TabSessionChat");
  __setInvokeForTest((async (command: string) => {
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
    if (command === "delayed_send_list") return [];
    return {};
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
  }) as any);
  __setListenForTest(async () => () => {});

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
  return { host, root };
}

function assertNoDeliveryUi(host: ParentNode): void {
  const text = host.textContent ?? "";
  assert(!/sending…|submission confirmed|submission unconfirmed|delivery failed/i.test(text), "no delivery copy");
  assert(
    !Array.from(host.querySelectorAll("button")).some((button) => /retry/i.test(button.textContent ?? "")),
    "no delivery Retry action",
  );
  assert(!host.querySelector('[role="alert"]'), "no delivery alert");
}

test("Chat has no delivery status surface", async () => {
  const { host, root } = await renderChat();

  assertNoDeliveryUi(host);

  await act(async () => root.unmount());
  host.remove();
});

test("TabSessionChat cannot subscribe to or render delivery telemetry", () => {
  const source = readFileSync(new URL("./TabSessionChat.tsx", import.meta.url), "utf8");
  assert(!source.includes("observeChatSendStatus"), "no delivery-status subscription");
  assert(!source.includes("ChatDeliveryState"), "no delivery-state renderer");
  assert(!source.includes("retryChatDelivery"), "no delivery Retry path");
});

test("the send boundary logs failures and has no delivery notification path", () => {
  const source = readFileSync(new URL("./ChatComposer.tsx", import.meta.url), "utf8");
  const sendStart = source.indexOf("async function send(");
  const sendEnd = source.indexOf("async function scheduleDraft", sendStart);
  const sendBody = source.slice(sendStart, sendEnd);

  assert(sendBody.includes('console.warn("[chat-send] optimistic delivery failed"'), "failure is logged");
  assert(
    !sendBody.includes("showComposerError(e instanceof Error ? e.message : String(e));"),
    "delivery failure never enters notification UI",
  );
  assert(
    !existsSync(new URL("./ChatDeliveryState.tsx", import.meta.url)),
    "delivery status component is deleted",
  );
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
