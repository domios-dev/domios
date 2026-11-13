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
import { addChatSend, readChatSends, failChatSendRequest, setChatSendDraft,
  __setChatSendRepositoryForTest, ensureChatSendTabHydrated, chatSendTabHydrationState, forgetChatSendsInMemory,
  observeChatSendDismissal, observeDeliveryStatus } from "./chatSendStore";
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

test("a local preparation failure exposes its retained reason under the original bubble", async () => {
  const tabId = "local-image-refusal";
  const { host, root } = await renderChat(tabId);
  try {
    await act(async () => {
      addChatSend(tabId, { id: "local-image", correlationId: "local-image", content: "inspect", sentAtMs: 1 });
      failChatSendRequest(tabId, "local-image", new Error("Image upload could not be read"));
    });
    const bubble = host.querySelector('[data-testid="user-bubble-row"]');
    assert(bubble?.querySelector('[role="status"]')?.getAttribute("aria-label")?.includes("Image upload could not be read"), "local reason reaches the dot");
  } finally { await act(async () => root.unmount()); host.remove(); }
});

async function openDeliveryActions(bubble: Element) {
  const status = bubble.querySelector('[role="status"]');
  assert(status?.textContent === "", "recovery stays hidden beside the bubble");
  const trigger = status.querySelector<HTMLButtonElement>("button");
  assert(trigger?.getAttribute("aria-label"), "the dot is an accessible button");
  await act(async () => trigger.click());
  assert(document.querySelector('[role="dialog"]'), "activating the dot opens recovery");
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

function tabState(tabId: string, runtimeState?: AgentRuntimeState, working = false): ExtendedState {
  const tab = {
    id: tabId,
    agent_runtime_state: runtimeState,
    ...(working ? { agent_state: { state: "working" } } : {}),
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
    tabStatuses: working ? { [tabId]: "Working" } : {},
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

async function renderChat(tabId = "delivery-tab", runtimeState?: AgentRuntimeState, sessionIntent?: { title: string; goal: string }, chatError?: string,
  seed: { messages?: unknown[]; working?: boolean; statuses?: unknown[]; general?: boolean } = {}): Promise<{
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
        messages: seed.messages ?? (sessionIntent ? [{ id: "intent", type: "custom", content: "", timestamp: 1, sessionIntent }] : []),
        source_session_id: "session-1",
        session_binding_nonce: "nonce-1",
        subscription_id: "delivery-subscription",
      };
    }
    if (command === "chat_send_status") return seed.statuses ?? [];
    if (command === "delayed_send_list" || command === "system_heartbeat_list") return [];
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
  const state = tabState(tabId, runtimeState, seed.working);
  if (seed.general) {
    state.tabs.general = { ...state.tabs[tabId], id: "general", protected: true };
    state.workspaces = [{ id: "general-workspace", protected: true, tabs: ["general"] }] as ExtendedState["workspaces"];
  }
  const store = createMuxStateStore(() => state);
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
      assert(![...(bubble()[0]?.querySelectorAll("button") ?? [])].some(button => button.textContent === "Retry"
        || button.textContent === "Edit and resend"), "held and written sends never offer replay");
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

test("runtime early user row keeps two image chips through source echo", async () => {
  const tabId = "runtime-image-delivery";
  const { host, root, dispatch } = await renderChat(tabId);
  const content = 'inspect <domios type="attachment" kind="image" path="/opaque/first.png" /> <domios type="attachment" kind="image" path="/opaque/second.png" />';
  const attachments = ["first", "second"].map(name => ({ id: name, mimeType: "image/png", path: `/opaque/${name}.png`, data: "AQID" }));
  const assertImages = () => {
    const bubbles = host.querySelectorAll('[data-testid="chat-message"][data-message-role="user"]');
    assert(bubbles.length === 1, "one user row retains both attachments");
    for (const name of ["first", "second"]) {
      assert(bubbles[0].querySelector(`[aria-label="Open image ${name}.png"]`), `${name} image remains accessible`);
    }
  };
  try {
    await act(async () => addChatSend(tabId, { id: "image-send", correlationId: "image-send", content, sentAtMs: Date.now() }));
    assertImages();
    for (const runtimeSourceItemId of [undefined, "provider-image-row"]) {
      await act(async () => dispatch(`tab-chat-msg-${tabId}`, {
        tab_id: tabId, source_session_id: "session-1", session_binding_nonce: "nonce-1",
        messages: [{ id: "image-send", type: "user", content, attachments,
          runtimeSourceItemId, correlationId: "image-send", timestamp: String(Date.now()) }],
      }));
      assertImages();
    }
  } finally {
    await act(async () => root.unmount()); host.remove();
  }
});

test("runtime image capability refusal is visible inside its retained bubble", async () => {
  const tabId = "runtime-image-refusal";
  const { host, root, dispatch } = await renderChat(tabId);
  try {
    await act(async () => addChatSend(tabId, { id: "refused-image", correlationId: "refused-image",
      content: 'inspect <domios type="attachment" kind="image" path="/opaque/image.png" />', sentAtMs: Date.now() }));
    const reason = "Provider refused: This ACP agent did not declare image support. Remove image attachments before sending.";
    await act(async () => dispatch("chat-send-status", { tab_id: tabId, send_id: "refused-image", correlationId: "refused-image",
      state: "failed", error: `tab_busy_refused: Tab ${tabId} refused delivery (nothing written): runtime_rejected_before_write: ${reason}`, sequence: 1, ledger_id: "ledger", retained_from: 1 }));
    const bubble = host.querySelector('[data-testid="chat-message"][data-message-role="user"]');
    assert(bubble?.querySelector('[role="status"]')?.getAttribute("aria-label")?.includes(reason), "the refusal reason reaches the accessible status");
    assert(bubble?.querySelector('[aria-label="Open image image.png"]'), "refused input retains its attachment chip");
  } finally { await act(async () => root.unmount()); host.remove(); }
});

test("a written prompt the agent refused while busy says so without offering Retry", async () => {
  const tabId = "runtime-busy-refusal";
  const { host, root, dispatch } = await renderChat(tabId);
  try {
    await act(async () => addChatSend(tabId, { id: "busy-refused", correlationId: "busy-refused",
      content: "opaque-busy-message", sentAtMs: Date.now() }));
    const reason = "opaque-refusal-reason";
    await act(async () => dispatch("chat-send-status", { tab_id: tabId, send_id: "busy-refused", correlationId: "busy-refused",
      state: "submit_unconfirmed", error: `agent_refused_while_busy: ${reason}`, sequence: 1, ledger_id: "ledger", retained_from: 1 }));
    const bubble = host.querySelector('[data-testid="chat-message"][data-message-role="user"]');
    const status = bubble?.querySelector('[role="status"]');
    assert(status?.getAttribute("data-state") === "uncertain", "the refused written prompt stays unconfirmed");
    assert(status?.getAttribute("aria-label")?.includes(reason), "the refusal reason reaches the accessible status");
    assert(status?.textContent === "", "the refused send has no visible label");
    assert(![...(bubble?.querySelectorAll("button") ?? [])].some((button) => button.textContent === "Retry"),
      "written input never offers Retry");
  } finally { await act(async () => root.unmount()); host.remove(); }
});

test("a held send waits after the running turn's live operation and becomes its one delivered bubble", async () => {
  const tabId = "held-behind-live-tool";
  const { __setInvokeForTest } = await import("../../lib/bridge");
  const turn = [
    { id: "turn-prompt", type: "user", content: "opaque-prompt" },
    { id: "turn-text", type: "assistant", content: "opaque-preamble" },
    { id: "turn-tool", type: "tool_call", content: "", toolId: "turn-tool", toolKind: "command", toolInput: { cmd: "opaque-command" },
      toolStatus: "running", toolStartedAtMs: Date.now() - 1000 },
  ];
  const { host, root, dispatch } = await renderChat(tabId, undefined, undefined, undefined, { messages: turn, working: true });
  const status = (state: "held" | "written" | "delivered", sequence: number) => ({
    tab_id: tabId, send_id: "held-tx", correlationId: "held-send", state, sequence,
    ledger_id: "ledger", retained_from: 1, error: null, pending_reason: state === "held" ? "held_active_turn" : undefined,
  });
  let latest = status("held", 1);
  __setInvokeForTest(async <T,>(command: string) => (command === "chat_send_status" ? [latest] : []) as T);
  const bubbles = () => host.querySelectorAll('[data-testid="chat-message"][data-message-role="user"]');
  try {
    await act(async () => addChatSend(tabId, { id: "held-send", correlationId: "held-send", content: "opaque-held", sentAtMs: Date.now() }));
    await act(async () => dispatch("chat-send-status", latest));
    const live = host.querySelectorAll('[data-testid="chat-live-operation"]');
    const held = host.querySelector('[data-message-id="held-send"]');
    assert(live.length === 1 && held, "one live operation and the held bubble render");
    assert(live[0].compareDocumentPosition(held) & Node.DOCUMENT_POSITION_FOLLOWING, "the held send follows the running tool");
    const rows = Array.from(host.querySelectorAll("[data-chat-window-row]"));
    assert(rows[rows.length - 1]?.contains(held), "the held send is the last transcript row");
    assert(!host.querySelector('[role="separator"]'), "an untimestamped turn gains no day divider from the held send");
    const queued = held.querySelector<HTMLElement>('[role="status"][data-state="queued"]');
    assert(queued?.getAttribute("aria-label"), "queued state exposes its reason");
    assert(queued?.textContent === "", "held input shows only the dot");
    assert(!queued.querySelector("button"), "held input has no delivery controls");

    latest = status("written", 2);
    await act(async () => dispatch("chat-send-status", latest));
    const written = held.querySelector('[role="status"][aria-label="Awaiting confirmation"]');
    assert(written?.textContent === "" && !written.querySelector("button"), "written input keeps a quiet accessible indicator");
    assert(live[0].compareDocumentPosition(held) & Node.DOCUMENT_POSITION_FOLLOWING,
      "a written send keeps its place after the turn it waited behind");
    latest = status("delivered", 3);
    await act(async () => dispatch("chat-send-status", latest));
    const delivered = Array.from(bubbles()).filter((bubble) => bubble.textContent?.includes("opaque-held"));
    assert(delivered.length === 1 && delivered[0].getAttribute("data-message-id") === "held-send", "delivery keeps the one bubble");
    const stillLive = host.querySelector('[data-testid="chat-live-operation"]');
    assert(stillLive && delivered[0].compareDocumentPosition(stillLive) & Node.DOCUMENT_POSITION_FOLLOWING,
      "confirmed delivery resumes conversation placement");
  } finally {
    await act(async () => root.unmount()); host.remove();
  }
});

const degradedRuntime: AgentRuntimeState = { state: "degraded", epoch: 2, failureCode: "provider_refused",
  reason: "opaque deadline exceeded; runtime recovery required" };

async function heldSendBeforeDegrade(tabId: string, repository: MemoryChatSendRepository) {
  __setChatSendRepositoryForTest(repository);
  await ensureChatSendTabHydrated(tabId);
  const held = { tab_id: tabId, send_id: "held-tx", correlationId: "held-send", state: "held" as const, sequence: 1,
    ledger_id: "ledger", retained_from: 1, error: null, pending_reason: "held_active_turn" };
  await act(async () => {
    addChatSend(tabId, { id: "held-send", correlationId: "held-send", content: "йо", sentAtMs: Date.now() });
    observeDeliveryStatus(held);
  });
  return held;
}
const heldBubble = (host: ParentNode) => host.querySelector('[data-testid="chat-message"][data-message-id="held-send"]');

test("a held send stays one Queued bubble across a degraded runtime and a reload", async () => {
  const tabId = "held-across-degrade";
  const repository = new MemoryChatSendRepository();
  await heldSendBeforeDegrade(tabId, repository);
  const first = await renderChat(tabId, { state: "loaded", epoch: 1 }, undefined, undefined, { messages: [{ id: "row", type: "assistant", content: "opaque" }] });
  try {
    await act(async () => first.dispatch("agent-runtime-state", { tab_id: tabId, session_binding_nonce: "nonce-1", state: degradedRuntime }));
    assert(heldBubble(first.host)?.querySelector('[role="status"][data-state="queued"]'), "the degrade keeps the Queued bubble");
  } finally {
    await act(async () => first.root.unmount()); first.host.remove();
  }
  await act(async () => { forgetChatSendsInMemory(); });
  const reloaded = await renderChat(tabId, degradedRuntime, undefined, undefined, { messages: [{ id: "row", type: "assistant", content: "opaque" }] });
  try {
    await act(async () => { await ensureChatSendTabHydrated(tabId); });
    const bubbles = reloaded.host.querySelectorAll('[data-message-id="held-send"]');
    assert(bubbles.length === 1, "the reload restores one bubble");
    const queued = bubbles[0]?.querySelector('[role="status"][data-state="queued"]');
    assert(queued, "the reloaded bubble still says Queued");
    assert(queued.getAttribute("aria-label") === "Queued for the agent", "a failed runtime is not described as a running turn");
    const rows = Array.from(reloaded.host.querySelectorAll("[data-chat-window-row]"));
    assert(rows[rows.length - 1]?.contains(bubbles[0]), "the held send is still the tail row");
  } finally {
    await act(async () => reloaded.root.unmount()); reloaded.host.remove();
    __setChatSendRepositoryForTest(null);
  }
});

test("a never-written send settled by a degrade offers Retry across a reload and leaves when dismissed", async () => {
  const tabId = "failed-across-reload";
  const repository = new MemoryChatSendRepository();
  const held = await heldSendBeforeDegrade(tabId, repository);
  const settled = { ...held, state: "failed" as const, sequence: 2, pending_reason: undefined, error: "launch_degraded_before_write" };
  await act(async () => observeDeliveryStatus(settled));
  await act(async () => { forgetChatSendsInMemory(); });
  const { host, root } = await renderChat(tabId, degradedRuntime, undefined, undefined, { statuses: [settled] });
  try {
    await act(async () => { await ensureChatSendTabHydrated(tabId); });
    const bubble = heldBubble(host);
    assert(bubble?.querySelector('[role="status"][data-state="failed"]'), "the reloaded bubble says it was not delivered");
    await openDeliveryActions(bubble!);
    assert(document.querySelector('[role="dialog"]')?.textContent?.includes("Retry"), "never-written input offers Retry");
    await act(async () => observeChatSendDismissal(tabId, { send_id: "held-tx", send: { id: "held-send", correlationId: "held-send", content: "йо", sentAtMs: 1 } }));
    assert(!heldBubble(host), "dismissal is what removes it");
  } finally {
    await act(async () => root.unmount()); host.remove();
    __setChatSendRepositoryForTest(null);
  }
});

test("a chat that cannot attach keeps undelivered bubbles beside its error and hides the rest", async () => {
  const tabId = "held-without-attachment";
  const repository = new MemoryChatSendRepository();
  await heldSendBeforeDegrade(tabId, repository);
  const { host, root } = await renderChat(tabId, degradedRuntime, undefined, "opaque-attach-refusal");
  try {
    await act(async () => { await Promise.resolve(); });
    const error = host.querySelector('[data-testid="chat-recovery-error"]');
    assert(error, "the attach failure is shown");
    const bubble = heldBubble(host);
    assert(bubble?.querySelector('[role="status"][data-state="queued"]'), "the Queued bubble survives the failed attach");
    const log = host.querySelector('[role="log"]');
    assert(log?.contains(error) && log.contains(bubble), "error and bubble are rows of one log, so neither covers the other");
    assert(bubble!.compareDocumentPosition(error) & Node.DOCUMENT_POSITION_FOLLOWING, "the error follows the bubble");
  } finally {
    await act(async () => root.unmount()); host.remove();
    __setChatSendRepositoryForTest(null);
  }
});

test("a held send from another window is at least counted", async () => {
  const tabId = "held-elsewhere";
  const repository = new MemoryChatSendRepository();
  __setChatSendRepositoryForTest(repository);
  await ensureChatSendTabHydrated(tabId);
  const { host, root } = await renderChat(tabId);
  const notice = () => host.querySelector('[data-testid="chat-unowned-held"]');
  try {
    assert(!notice(), "nothing is queued yet");
    const held = { tab_id: tabId, send_id: "other-tx", correlationId: "other-token", state: "held" as const, sequence: 1,
      ledger_id: "ledger", retained_from: 1, error: null };
    await act(async () => observeDeliveryStatus(held));
    assert(notice()?.getAttribute("role") === "status", "the owner can see something is pending");
    await act(async () => observeDeliveryStatus({ ...held, state: "written", sequence: 2 }));
    assert(!notice(), "the notice ends with the hold");
  } finally {
    await act(async () => root.unmount()); host.remove();
    __setChatSendRepositoryForTest(null);
  }
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
    await openDeliveryActions(bubble()[0]);
    const retry = Array.from(document.querySelectorAll("button")).find((button) => button.textContent === "Retry");
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
    const { host, root } = await renderChat(tabId, undefined, undefined, undefined, { general: true });
    const { __setInvokeForTest } = await import("../../lib/bridge");
    const { prepareChatDelivery } = await import("./chatDeliveryPreparation");
    const sends: Array<{ tabId: string; text: string }> = [];
    let focused: unknown;
    const onFocus = (event: Event) => { focused = (event as CustomEvent).detail; };
    window.addEventListener("codeterm:focus-tab", onFocus);
    __setInvokeForTest(async <T,>(command: string, args?: Record<string, unknown>) => {
      if (command === "transcribe_voice_attachment") throw new Error("transcriber is unavailable");
      if (command === "chat_send_text") sends.push(args as { tabId: string; text: string });
      return [] as T;
    });
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
        await prepareChatDelivery(tabId, readChatSends(tabId).find(send => send.id === "voice")!, text => ({ kind: "expanded", text }))
          .then(() => { throw new Error("transcription should fail"); }, () => {});
      });
      assert(!bubble.querySelector('[aria-busy="true"]'), "failure removes progress from the same bubble");
      const status = bubble.querySelector('[role="status"]');
      const buttons = [...(status?.querySelectorAll("button") ?? [])];
      assert(buttons.length === 1 && buttons[0].textContent === "Set up transcriber", "transcription failure offers only setup");
      assert(status?.textContent?.includes("transcriber is unavailable"), "transcription reason remains visible");
      await act(async () => buttons[0].click());
      assert(sends.length === 1 && sends[0].tabId === "general" && sends[0].text.includes("transcriber"), "setup sends one request to General");
      assert((focused as { tabId: string; workspaceId: string })?.tabId === "general"
        && (focused as { workspaceId: string })?.workspaceId === "general-workspace", "setup brings General forward");
      assert(readChatSends(tabId).find(send => send.id === "voice")?.requestFailed, "setup leaves original failure unreplayed");
      await act(async () => failChatSendRequest(tabId, "voice", "Transport unavailable"));
      await openDeliveryActions(bubble);
      const transportControls = [...document.querySelectorAll('[role="dialog"] button')].map(button => button.textContent);
      assert(transportControls.includes("Retry") && transportControls.includes("Remove")
        && !transportControls.includes("Set up transcriber"), "ordinary voice send failures retain delivery recovery");
    } finally {
      await act(async () => root.unmount());
      host.remove();
      window.matchMedia = originalMatchMedia;
      window.removeEventListener("codeterm:focus-tab", onFocus);
      __setInvokeForTest(null);
    }
  });
}

test("hydrated tokenless input exposes recovery and stays removed after reload", async () => {
  const tabId = "hydrated-tokenless";
  const repository = new MemoryChatSendRepository();
  repository.records.set(JSON.stringify([tabId, "tokenless"]), encodeChatSend(tabId, {
    id: "tokenless", content: "opaque stored input", sentAtMs: 1,
  }));
  __setChatSendRepositoryForTest(repository);
  const { host, root } = await renderChat(tabId);
  const { __setInvokeForTest } = await import("../../lib/bridge");
  let requests = 0;
  __setInvokeForTest(async <T,>() => { requests++; return [] as T; });
  try {
    await act(async () => { await ensureChatSendTabHydrated(tabId); });
    const bubble = host.querySelector('[data-message-id="tokenless"]');
    assert(bubble && bubble.querySelector('[role="status"][data-state="uncertain"]'), "restored input has a reason");
    await openDeliveryActions(bubble);
    const buttons = [...document.querySelectorAll<HTMLButtonElement>('[role="dialog"] button')];
    assert(buttons.some(button => button.textContent === "Edit and resend"), "restored input is editable");
    assert(!buttons.some(button => button.textContent === "Retry"), "restored input never offers replay");
    const remove = buttons.find(button => button.textContent === "Remove");
    assert(remove, "restored input is removable");
    await act(async () => remove.click());
    assert(!host.querySelector('[data-message-id="tokenless"]'), "local receipt removes the bubble");
    await act(async () => { forgetChatSendsInMemory(); await ensureChatSendTabHydrated(tabId); });
    assert(!host.querySelector('[data-message-id="tokenless"]'), "removal survives rehydration");
    assert(requests === 0, "local recovery sends no host request");
  } finally {
    __setInvokeForTest(null);
    await act(async () => root.unmount()); host.remove();
    __setChatSendRepositoryForTest(null);
  }
});

test("failed image send with an early runtime row renders one bubble with visible recovery", async () => {
  const tabId = "failed-image-identity";
  const { host, root, dispatch } = await renderChat(tabId);
  const content = 'inspect <domios type="attachment" kind="image" path="/opaque/first.png" /> <domios type="attachment" kind="image" path="/opaque/second.png" />';
  try {
    await act(async () => addChatSend(tabId, { id: "image-send", correlationId: "image-send", content, sentAtMs: Date.now() }));
    await act(async () => dispatch(`tab-chat-msg-${tabId}`, {
      tab_id: tabId, source_session_id: "session-1", session_binding_nonce: "nonce-1",
      messages: [{ id: "image-send", type: "user", content, correlationId: "image-send", timestamp: String(Date.now()),
        attachments: ["first", "second"].map(name => ({ id: name, mimeType: "image/png", path: `/opaque/${name}.png`, data: "AQID" })) }],
    }));
    await act(async () => dispatch("chat-send-status", { tab_id: tabId, send_id: "image-tx", correlationId: "image-send",
      state: "failed", error: null, sequence: 1, ledger_id: "ledger", retained_from: 1 }));
    const bubbles = host.querySelectorAll('[data-testid="chat-message"][data-message-role="user"]');
    assert(bubbles.length === 1, "same identity stays one bubble after failure");
    await openDeliveryActions(bubbles[0]);
    const controls = [...document.querySelectorAll('[role="dialog"] button')].map(button => button.textContent);
    assert(controls.includes("Retry") && controls.includes("Remove"), "failure offers both visible exits");
    assert(bubbles[0].querySelector('[aria-label="Open image first.png"]'), "first image remains");
    assert(bubbles[0].querySelector('[aria-label="Open image second.png"]'), "second image remains");
  } finally { await act(async () => root.unmount()); host.remove(); }
});

test("Edit and resend restores text and images without sending and submits a fresh identity", async () => {
  const tabId = "edit-uncertain-images";
  const { host, root, dispatch } = await renderChat(tabId);
  const { __setInvokeForTest } = await import("../../lib/bridge");
  let requests = 0;
  let newToken: unknown;
  const content = 'inspect <domios type="attachment" kind="image" path="/opaque/first.png" /> <domios type="attachment" kind="image" path="/opaque/second.png" />';
  try {
    await act(async () => setAgentDescriptors([{ id: "lmstudio", attachments: { imagePaths: true, filePaths: true, inlineText: true },
      agent_runtime: { goals: { actions: ["read"] } } }] as unknown as ProviderDescriptor[]));
    await act(async () => addChatSend(tabId, { id: "original", correlationId: "original", content, sentAtMs: Date.now() }));
    await act(async () => dispatch("chat-send-status", { tab_id: tabId, send_id: "original-tx", correlationId: "original",
      state: "submit_unconfirmed", error: null, sequence: 1, ledger_id: "ledger", retained_from: 1 }));
    __setInvokeForTest(async <T,>(command: string, args?: Record<string, unknown>) => {
      if (command === "chat_send_text") {
        requests++;
        newToken = args?.clientToken;
        assert(String(args?.text).includes("first.png") && String(args?.text).includes("second.png"), "new payload retains both attachments");
        return { tab_id: tabId, send_id: "new-tx", correlationId: newToken, state: "written",
          error: null, sequence: 2, ledger_id: "ledger", retained_from: 1 } as T;
      }
      return [] as T;
    });
    await openDeliveryActions(host.querySelector('[data-message-id="original"]')!);
    const edit = [...document.querySelectorAll("button")].find(button => button.textContent === "Edit and resend");
    assert(edit, "uncertain bubble exposes edit");
    assert(![...host.querySelectorAll("button")].some(button => button.textContent === "Retry"), "no replay control");
    await act(async () => edit.click());
    const editor = host.querySelector<HTMLTextAreaElement>('textarea[aria-label="Message composer"]');
    assert(editor && editor.value.includes("inspect"), "text restored");
    assert(requests === 0, "editing performs no send");
    await act(async () => editor.dispatchEvent(new window.KeyboardEvent("keydown", { key: "Enter", code: "Enter", bubbles: true })));
    assert(Number(requests) === 1, "ordinary submit writes once");
    assert(newToken !== "original" && typeof newToken === "string", "new send has its own token");
    assert(readChatSends(tabId).some(send => send.id === "original" && send.status?.state === "submit_unconfirmed"), "original never replays");
  } finally { __setInvokeForTest(null); await act(async () => root.unmount()); host.remove(); }
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

