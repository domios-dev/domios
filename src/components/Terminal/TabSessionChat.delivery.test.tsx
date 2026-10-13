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
import { addChatSend, readChatSends, updateChatSendContent, failChatSendRequest, setChatSendDraft } from "./chatSendStore";

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
  Object.defineProperty(window.HTMLElement.prototype, "clientHeight", { configurable: true, get: () => 900 });
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
    agent_session_id: "session-1",
    session_binding_nonce: "nonce-1",
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
    answerNotification: noop,
    createWorkspace: noopAsync,
    setActiveWorkspace: noop,
    closeWorkspace: noop,
    renameWorkspace: noop,
    moveTab: noop,
    reorderTabs: noop,
    reorderDisplay: noop,
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

type ChatEventHandler = (event: { payload: unknown }) => void;

async function renderChat(tabId = "delivery-tab"): Promise<{
  host: HTMLDivElement;
  root: Root;
  dispatch: (event: string, payload: unknown) => void;
}> {
  const { __setInvokeForTest, __setListenForTest } = await import("../../lib/bridge");
  const { TabSessionChat } = await import("./TabSessionChat");
  __setInvokeForTest((async (command: string) => {
    if (command === "chat_start") {
      return {
        text: "",
        offset: 0,
        messages: [],
        source_session_id: "session-1",
        session_binding_nonce: "nonce-1",
        subscription_id: "delivery-subscription",
      };
    }
    if (command === "delayed_send_list" || command === "chat_send_status") return [];
    return {};
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
  }) as any);
  const listeners = new Map<string, Set<ChatEventHandler>>();
  __setListenForTest(async <T,>(event: string, typedHandler: (e: { payload: T }) => void) => {
    const handler = typedHandler as unknown as ChatEventHandler;
    const set = listeners.get(event) ?? new Set<ChatEventHandler>();
    set.add(handler);
    listeners.set(event, set);
    return () => { set.delete(handler); };
  });

  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  const store = createMuxStateStore(() => tabState(tabId));
  await act(async () => {
    root.render(
      <MuxStateStoreContext.Provider value={store}>
        <MultiplexerActionsContext.Provider value={muxActions()}>
          <TabSessionChat tabId={tabId} chatBackend isVisible />
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
    dispatch: (event, payload) => {
      for (const handler of listeners.get(event) ?? []) handler({ payload });
    },
  };
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

test("three optimistic sends reconcile to ordered parts of one provider turn", async () => {
  const tabId = "merged-delivery-tab";
  const { host, root, dispatch } = await renderChat(tabId);
  const bodies = ["opaque-first", "opaque-second", "opaque-third"];
  const sentAtMs = Date.now();

  await act(async () => {
    bodies.forEach((content, index) => addChatSend(tabId, {
      id: `send-${index}`,
      content,
      sentAtMs: sentAtMs + index,
      correlationId: `send-${index}`,
    }));
  });
  await act(async () => {
    dispatch(`tab-chat-msg-${tabId}`, {
      tab_id: tabId,
      source_session_id: "session-1",
      session_binding_nonce: "nonce-1",
      messages: [{
        id: "merged-provider-turn",
        type: "user",
        content: bodies.join(""),
        timestamp: String(sentAtMs + 100),
        correlationId: "send-0",
      }],
    });
    await Promise.resolve();
    await Promise.resolve();
  });

  const bubbles = host.querySelectorAll('[data-testid="chat-message"][data-message-role="user"]');
  assert(bubbles.length === bodies.length, "merged turn renders each supplied part once");
  bodies.forEach((body, index) => {
    assert(bubbles[index]?.textContent?.includes(body), "ordered parts retain their supplied payload");
    assert(bubbles[index]?.getAttribute("data-message-id") !== `send-${index}`, "committed parts replace optimistic rows");
  });

  await act(async () => root.unmount());
  host.remove();
});

test("one bubble follows event state and Retry reuses it", async () => {
  const tabId = "bubble-delivery-states";
  const { __setInvokeForTest } = await import("../../lib/bridge");
  const { host, root, dispatch } = await renderChat(tabId);
  const status = (state: "held" | "written" | "failed" | "delivered", sequence: number, send_id = "tx-one") => ({
    tab_id: tabId, send_id, state, sequence, ledger_id: "ledger", retained_from: 1,
    error: null, correlationId: readChatSends(tabId)[0]?.correlationId ?? "bubble-one",
  });
  let latest = status("held", 1);
  let requests = 0;
  __setInvokeForTest(async <T,>(command: string) => {
    if (command === "chat_send_status") return [latest] as T;
    if (command === "chat_send_text") { requests++; latest = status("written", 5, "tx-two"); return latest as T; }
    return [] as T;
  });
  try {
    await act(async () => addChatSend(tabId, { id: "bubble-one", correlationId: "bubble-one", content: "opaque-message", sentAtMs: Date.now() }));
    const bubble = () => host.querySelectorAll('[data-testid="chat-message"][data-message-role="user"]');
    assert(bubble().length === 1, "one optimistic row");
    assert(host.querySelector('[role="status"]')?.getAttribute("aria-live") === "polite", "subtle accessible status");
    for (const [state, sequence] of [["held", 2], ["failed", 4]] as const) {
      latest = status(state, sequence);
      await act(async () => { dispatch("chat-send-status", latest); });
      assert(bubble().length === 1, "state changes never add a row");
    }
    const retry = Array.from(host.querySelectorAll("button")).find((button) => button.textContent === "Retry");
    assert(retry, "failed send offers Retry");
    await act(async () => { retry.click(); retry.click(); });
    assert(requests === 1, "Retry admits once");
    assert(readChatSends(tabId).length === 1, "Retry keeps one send record");
    assert(bubble()[0]?.getAttribute("data-message-id") === "bubble-one", "Retry retains bubble identity");
    latest = status("delivered", 6, "tx-two");
    await act(async () => { dispatch("chat-send-status", latest); });
    assert(!Array.from(host.querySelectorAll("button")).some((button) => button.textContent === "Retry"), "delivery removes Retry");
    await act(async () => dispatch(`tab-chat-msg-${tabId}`, {
      tab_id: tabId, source_session_id: "session-1", session_binding_nonce: "nonce-1",
      messages: [{ id: "provider-echo", type: "user", content: "opaque-message", timestamp: String(Date.now()), correlationId: latest.correlationId }],
    }));
    assert(bubble().length === 1, "provider echo reconciles to one row");
  } finally {
    await act(async () => root.unmount()); host.remove();
  }
});

for (const reducedMotion of [false, true]) {
  test(`voice progress is wired inside its bubble (reduced motion: ${reducedMotion})`, async () => {
    const originalMatchMedia = window.matchMedia;
    window.matchMedia = (query) => ({
      media: query, matches: reducedMotion, onchange: null,
      addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {},
      dispatchEvent: () => true,
    });
    const tabId = `voice-progress-${reducedMotion}`;
    const { host, root } = await renderChat(tabId);
    try {
      await act(async () => {
        addChatSend(tabId, {
          id: "voice", correlationId: "voice", content: "opaque-placeholder", sentAtMs: Date.now(),
        });
        setChatSendDraft(tabId, "voice", {
          text: "", attachments: ["/voice.wav"], recordedVoiceAttachments: ["/voice.wav"], capsules: {}, reply: null,
        });
      });
      const bubble = host.querySelector('[data-testid="chat-message"][data-message-id="voice"]')!;
      const progress = bubble.querySelector('[role="status"][aria-busy="true"]');
      assert(progress?.getAttribute("aria-label"), "progress has an accessible name");
      assert(progress?.getAttribute("data-motion") === (reducedMotion ? "reduced" : "animated"), "motion preference reaches progress");
      await act(async () => {
        updateChatSendContent(tabId, "voice", "opaque-failure");
        failChatSendRequest(tabId, "voice");
      });
      assert(!bubble.querySelector('[aria-busy="true"]'), "failure removes progress from the same bubble");
      assert(bubble.querySelector('button'), "failed voice retains its retry control");
    } finally {
      await act(async () => root.unmount());
      host.remove();
      window.matchMedia = originalMatchMedia;
    }
  });
}

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
