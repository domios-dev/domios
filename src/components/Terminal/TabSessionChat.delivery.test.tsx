import { act } from "react";
import { Window } from "happy-dom";
import type { Root } from "react-dom/client";
import type { AgentRuntimeState } from "../../lib/agentRuntimeState";
import type { Tab } from "../../lib/types";
import type { ProviderDescriptor } from "../../lib/types";
import { setAgentDescriptors } from "../../lib/agentDescriptorCache";
import {
  MultiplexerActionsContext,
  MuxStateStoreContext,
  type MultiplexerActionsContextType,
} from "../../store/multiplexer/context";
import { createMuxStateStore } from "../../store/multiplexer/selectorStore";
import type { ExtendedState } from "../../store/multiplexer/tree";
import { addChatSend, readChatSends, updateChatSendContent, failChatSendRequest, setChatSendDraft,
  __setChatSendRepositoryForTest, ensureChatSendTabHydrated, chatSendTabHydrationState } from "./chatSendStore";
import { encodeChatSend } from "./chatSendDurableModel";
import { MemoryChatSendRepository } from "./chatSendRepository.testSupport";
import { sendTrackedChatMessage } from "./chatSendStatus";

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

function tabState(tabId: string, runtimeState?: AgentRuntimeState): ExtendedState {
  const tab = {
    id: tabId,
    agent_runtime_state: runtimeState,
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
    setWorkspaceCwd: async () => ({ warnings: [] }),
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
    upsertToast: noop,
    markRead: noop,
    markAllRead: noop,
    deleteNotification: noop,
    clearNotifications: noop,
    dismissToast: noop,
    renameTab: noop,
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

async function renderChat(tabId = "delivery-tab", runtimeState?: AgentRuntimeState, sessionIntent?: { title: string; goal: string }, chatError?: string): Promise<{
  host: HTMLDivElement;
  root: Root;
  dispatch: (event: string, payload: unknown) => void;
  calls: string[];
}> {
  setAgentDescriptors([{ id: "lmstudio", agent_runtime: { goals: { actions: ["read"] } } }] as unknown as ProviderDescriptor[]);
  const { __setInvokeForTest, __setListenForTest } = await import("../../lib/bridge");
  const { createRoot } = await import("react-dom/client");
  const { TabSessionChat } = await import("./TabSessionChat");
  const calls: string[] = [];
  __setInvokeForTest((async (command: string) => {
    calls.push(command);
    if (command === "agent_runtime_goal") return { supported: true, intent: sessionIntent, actions: [] };
    if (command === "agent_runtime_retry") return { state: "loaded", epoch: 3 };
    if (command === "chat_start") {
      if (chatError) throw new Error(chatError);
      return {
        text: "",
        offset: 0,
        messages: sessionIntent ? [{ id: "intent", type: "custom", content: "", timestamp: 1, sessionIntent }] : [],
        source_session_id: "session-1",
        session_binding_nonce: "nonce-1",
        subscription_id: "delivery-subscription",
      };
    }
    if (command === "delayed_send_list" || command === "system_heartbeat_list" || command === "chat_send_status") return [];
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
  const store = createMuxStateStore(() => tabState(tabId, runtimeState));
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
    calls,
    dispatch: (event, payload) => {
      for (const handler of listeners.get(event) ?? []) handler({ payload });
    },
  };
}


for (const narrow of [false, true]) {
  test(`session goal is reachable in the composer (narrow: ${narrow})`, async () => {
    const originalMatchMedia = window.matchMedia;
    window.matchMedia = (query) => ({
      media: query, matches: narrow, onchange: null,
      addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {}, dispatchEvent: () => true,
    });
    const intent = { title: "fixture-title", goal: "fixture-goal" };
    const { host, root } = await renderChat("delivery-tab", undefined, intent);
    try {
      const composer = host.querySelector('[data-testid="composer-context-header"]');
      const goal = composer?.querySelector<HTMLButtonElement>('button[aria-label="Session goal"]');
      assert(goal, "composer exposes the session goal control");
      assert(goal.closest('[role="group"][aria-label="Linked to this tab"]'), "goal shares the linked controls row");
      assert(!host.querySelector('[role="group"][aria-label="Session intent"]'), "chat has no intent strip");
      assert(!host.querySelector('[role="log"] [aria-label="Session goal"]'), "goal is outside the transcript");
      await act(async () => { goal.click(); });
      const details = host.querySelector('[role="dialog"][aria-label="Session goal"]');
      assert(details?.textContent?.includes(intent.title) && details.textContent.includes(intent.goal), "opening the goal reveals both supplied values");
      await act(async () => { document.dispatchEvent(new KeyboardEvent("keydown", { code: "Escape", bubbles: true })); });
      assert(!host.querySelector('[role="dialog"][aria-label="Session goal"]'), "Escape dismisses the goal");
    } finally {
      await act(async () => { root.unmount(); });
      host.remove();
      window.matchMedia = originalMatchMedia;
    }
  });
}

test("removed plugin details are reachable from the composer status bubble", async () => {
  const error = JSON.stringify({ code: "chat_backend_plugin_removed", plugin_id: "fixture-plugin" });
  const { host, root } = await renderChat("delivery-tab", undefined, undefined, error);
  try {
    const composer = host.querySelector('[data-testid="composer-context-header"]');
    const status = composer?.querySelector<HTMLButtonElement>('button[aria-label="Plugin removed"]');
    assert(status, "composer exposes the removed plugin status");
    assert(status.textContent === "", "plugin status trigger is icon-only");
    assert(host.querySelectorAll('[aria-label="Plugin removed"]').length === 1, "no persistent removed plugin notice exists outside the bubble");
    await act(async () => { status.click(); });
    const details = document.querySelector('[role="dialog"][aria-label="Plugin removed"]');
    assert(details?.querySelector('[aria-label="Plugin removed"]'), "status opens the existing error notice with its details and actions");
    await act(async () => { document.dispatchEvent(new KeyboardEvent("keydown", { code: "Escape", bubbles: true })); });
    assert(!document.querySelector('[role="dialog"][aria-label="Plugin removed"]'), "Escape dismisses plugin details");
  } finally {
    await act(async () => { root.unmount(); });
    host.remove();
  }
});

for (const touch of [false, true]) {
  for (const state of ["degraded", "unloaded", "loading"] as const) {
    test(`runtime ${state} uses the shared recovery surface (touch: ${touch})`, async () => {
      const originalMatchMedia = window.matchMedia;
      window.matchMedia = (query) => ({
        media: query, matches: touch && (query === "(pointer: coarse)" || query === "(hover: none)"), onchange: null,
        addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {}, dispatchEvent: () => true,
      });
      const tabId = `runtime-${state}-${touch}`;
      const { host, root, calls, dispatch } = await renderChat(tabId, { state, epoch: 2, reason: "opaque-overflow" });
      try {
        const badge = host.querySelector<HTMLButtonElement>('button[aria-haspopup="dialog"]');
        assert(badge, "runtime status badge is mounted");
        await act(async () => badge.click());
        const retry = document.querySelector<HTMLButtonElement>('button[aria-label="Retry"]');
        assert(retry, "runtime recovery action is mounted");
        assert(host.querySelector('textarea'), "composer remains available during runtime recovery");
        assert(retry.disabled === (state === "loading"), "only pending recovery disables Retry");
        if (state !== "loading") {
          await act(async () => { retry.click(); });
          assert(calls.filter(command => command === "agent_runtime_retry").length === 1, "Retry reaches the runtime once");
        } else {
          await act(async () => dispatch("agent-runtime-state", {
            tab_id: tabId, session_binding_nonce: "nonce-1", state: { state: "loaded", epoch: 3 },
          }));
        }
        assert(!host.querySelector('button[aria-haspopup="dialog"]'), "loaded runtime removes recovery badge");
        assert(!document.querySelector('[role="dialog"]'), "loaded runtime removes open popover");
        assert(host.querySelector('textarea'), "loaded runtime restores the shared composer");
      } finally {
        await act(async () => root.unmount()); host.remove();
        window.matchMedia = originalMatchMedia;
      }
    });
  }
}

test("mobile held and written sends retain one bubble beside runtime recovery", async () => {
  const originalMatchMedia = window.matchMedia;
  window.matchMedia = (query) => ({
    media: query, matches: true, onchange: null,
    addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {}, dispatchEvent: () => true,
  });
  const tabId = "mobile-held-send";
  const { __setInvokeForTest } = await import("../../lib/bridge");
  const { host, root, dispatch } = await renderChat(tabId, { state: "degraded", epoch: 2, reason: "opaque-fault" });
  try {
    await act(async () => addChatSend(tabId, { id: "mobile-send", correlationId: "mobile-send", content: "opaque-input", sentAtMs: Date.now() }));
    const bubble = () => host.querySelectorAll('[data-message-role="user"]');
    for (const [state, sequence] of [["held", 1], ["written", 2]] as const) {
      const status = {
        tab_id: tabId, send_id: "mobile-tx", correlationId: "mobile-send", state, sequence,
        ledger_id: "ledger", retained_from: 1, error: null,
      };
      __setInvokeForTest(async <T,>(command: string) => (command === "chat_send_status" ? [status] : []) as T);
      await act(async () => dispatch("chat-send-status", status));
      assert(bubble().length === 1, "mobile delivery updates one row");
      assert(bubble()[0]?.querySelector('[role="status"]'), "shared delivery model reaches the bubble");
      assert(!bubble()[0]?.querySelector("button"), "held and written sends never offer replay");
      assert(host.querySelector('button[aria-haspopup="dialog"]'), "runtime recovery remains available");
    }
  } finally {
    await act(async () => root.unmount()); host.remove();
    window.matchMedia = originalMatchMedia;
  }
});

test("mobile runtime sign-in uses the shared account card", async () => {
  const originalMatchMedia = window.matchMedia;
  window.matchMedia = (query) => ({
    media: query, matches: true, onchange: null,
    addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {}, dispatchEvent: () => true,
  });
  const { host, root } = await renderChat("mobile-login", {
    state: "unloaded", epoch: 2, auth: "signedOut", loginMethods: ["apiKey"], loginProviderName: "Fixture",
  });
  try {
    const card = host.querySelector('[role="group"][aria-label="Sign in to Fixture"]');
    assert(card, "provider account card is mounted");
    assert(host.querySelector('textarea'), "sign-in keeps composer available");
    const keyButton = Array.from(card.querySelectorAll("button")).find(button => button.textContent?.includes("API key"));
    assert(keyButton, "provider method is available");
    await act(async () => keyButton.click());
    assert(card.querySelector('input[type="password"][aria-label="API key"]'), "API key stays masked");
  } finally {
    await act(async () => root.unmount()); host.remove();
    window.matchMedia = originalMatchMedia;
  }
});

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

test("composer accepts a live send during hydration and keeps its newer local record", async () => {
  const tabId = "send-during-hydration";
  let finishHydration!: () => void;
  let finishPersistence!: () => void;
  const repository = new MemoryChatSendRepository();
  repository.hydrationGate = new Promise<void>((resolve) => { finishHydration = resolve; });
  repository.persistGate = new Promise<void>((resolve) => { finishPersistence = resolve; });
  repository.records.set(JSON.stringify([tabId, "live-send"]), encodeChatSend(tabId, {
    id: "live-send", correlationId: "live-send", content: "older persisted value", sentAtMs: 1,
  }));
  __setChatSendRepositoryForTest(repository);
  const { host, root } = await renderChat(tabId);
  try {
    const editor = host.querySelector<HTMLTextAreaElement>('textarea[aria-label="Message composer"]');
    assert(editor, "composer stays available while the ledger is pending");
    assert(chatSendTabHydrationState(tabId) === "pending", "ledger remains explicitly pending");
    const setValue = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")?.set;
    await act(async () => {
      setValue?.call(editor, "local edit during hydration");
      editor.dispatchEvent(new InputEvent("input", { bubbles: true, inputType: "insertText", data: "local edit during hydration" }));
      editor.dispatchEvent(new Event("change", { bubbles: true }));
    });
    assert(editor.value === "local edit during hydration", "typing updates immediately while persistence is pending");
    assert(host.querySelector('textarea[aria-label="Message composer"]') === editor, "input keeps the mounted composer node");
    await act(async () => addChatSend(tabId, {
      id: "live-send", correlationId: "live-send", content: "current live value", sentAtMs: 2,
    }));
    assert(host.textContent?.includes("current live value"), "live optimistic row renders during hydration");
    assert(editor.value === "local edit during hydration", `live row update keeps the local draft (${editor.value})`);

    const { __setInvokeForTest } = await import("../../lib/bridge");
    const status = {
      tab_id: tabId, send_id: "transaction-live", correlationId: "live-send", state: "written" as const,
      sequence: 1, ledger_id: "ledger", retained_from: 1, error: null,
    };
    let sendRequests = 0;
    __setInvokeForTest(async <T,>(command: string) => {
      if (command === "chat_send_text") { sendRequests++; return status as T; }
      if (command === "chat_send_status") return [status] as T;
      return [] as T;
    });
    await act(async () => {
      await sendTrackedChatMessage(tabId, "live-send", "current live value");
    });
    assert(sendRequests === 1, "send admission is not gated on persistence hydration");
    assert(host.querySelector('textarea[aria-label="Message composer"]') === editor, "send status keeps the composer mounted");
    assert(editor.value === "local edit during hydration", `send status keeps the local draft (${editor.value})`);

    await act(async () => {
      finishHydration();
      await ensureChatSendTabHydrated(tabId);
    });
    const live = readChatSends(tabId).find((send) => send.id === "live-send");
    assert(live?.content === "current live value", "late hydration does not replace the current live send");
    assert(live.status?.state === "written", "late hydration preserves the live delivery evidence");
    assert(chatSendTabHydrationState(tabId) === "ready", "successful hydration reaches ready");
    assert(host.querySelector('textarea[aria-label="Message composer"]') === editor, "hydration keeps the focused composer node");
    assert(editor.value === "local edit during hydration", `hydration preserves the current composer draft (${editor.value})`);
  } finally {
    finishHydration();
    finishPersistence();
    await act(async () => root.unmount());
    host.remove();
    __setChatSendRepositoryForTest(null);
  }
});

test("failed ledger hydration leaves the chat composer and send path usable", async () => {
  const tabId = "send-after-hydration-error";
  const repository = new MemoryChatSendRepository();
  repository.failHydration = true;
  __setChatSendRepositoryForTest(repository);
  const { host, root } = await renderChat(tabId);
  try {
    await act(async () => ensureChatSendTabHydrated(tabId));
    assert(chatSendTabHydrationState(tabId) === "error", "storage failure is exposed as an error state");
    const editor = host.querySelector<HTMLTextAreaElement>('textarea[aria-label="Message composer"]');
    assert(editor, "storage failure does not remove the composer");
    const setValue = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")?.set;
    await act(async () => {
      setValue?.call(editor, "draft after storage error");
      editor.dispatchEvent(new InputEvent("input", { bubbles: true, inputType: "insertText", data: "draft after storage error" }));
      editor.dispatchEvent(new Event("change", { bubbles: true }));
    });
    await act(async () => addChatSend(tabId, {
      id: "still-live", correlationId: "still-live", content: "usable after error", sentAtMs: 3,
    }));
    assert(host.textContent?.includes("usable after error"), "current-session send row remains visible after storage error");
    assert(readChatSends(tabId).find((send) => send.id === "still-live")?.status == null,
      "storage failure does not invent a delivery result");
    const { __setInvokeForTest } = await import("../../lib/bridge");
    let sendRequests = 0;
    __setInvokeForTest(async <T,>(command: string) => {
      if (command === "chat_send_text") {
        sendRequests++;
        return { tab_id: tabId, send_id: "still-live", correlationId: "still-live", state: "written",
          sequence: 1, ledger_id: "ledger", retained_from: 1, error: null } as T;
      }
      return [] as T;
    });
    await act(async () => { await sendTrackedChatMessage(tabId, "still-live", "usable after error"); });
    assert(sendRequests === 1, "storage failure does not gate live send admission");
    assert(host.querySelector('textarea[aria-label="Message composer"]') === editor, "storage failure keeps the same composer node");
    assert(editor.value === "draft after storage error", `storage failure preserves the local draft (${editor.value})`);
  } finally {
    await act(async () => root.unmount());
    host.remove();
    __setChatSendRepositoryForTest(null);
  }
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

