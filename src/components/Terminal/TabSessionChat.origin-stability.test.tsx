// After the fix those props are memoized per message id and stay referentially stable, so only the actively-changing tail re-renders per chunk.

import { act } from "react";
import { Window } from "happy-dom";
import { createRoot } from "react-dom/client";
import { ChatBlockHostProvider } from "@codeterm/chat-ui";
import type { Tab } from "../../lib/types";
import { MultiplexerActionsContext, MuxStateStoreContext, type MultiplexerActionsContextType } from "../../store/multiplexer/context";
import { createMuxStateStore } from "../../store/multiplexer/selectorStore";
import type { ExtendedState } from "../../store/multiplexer/tree";

type TestFn = () => void | Promise<void>;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) { tests.push([name, fn]); }
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

type Handler = (e: { payload: unknown }) => void;
const listeners = new Map<string, Set<Handler>>();
function emit(event: string, payload: unknown) {
  for (const cb of [...(listeners.get(event) ?? [])]) cb({ payload });
}

function setupDom() {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const window = new Window({ url: "http://localhost/" });
  for (const key of ["window", "document", "navigator", "HTMLElement", "HTMLTextAreaElement", "SVGElement", "Element", "Node", "Event", "CustomEvent"] as const) {
    Object.defineProperty(globalThis, key, { value: (window as never)[key], configurable: true });
  }
  Object.defineProperty(globalThis, "requestAnimationFrame", { configurable: true, value: (cb: FrameRequestCallback) => { cb(0); return 0; } });
  Object.defineProperty(globalThis, "cancelAnimationFrame", { configurable: true, value: () => {} });
  Object.defineProperty(window.HTMLElement.prototype, "clientHeight", {
    configurable: true,
    get(this: HTMLElement) { return this.dataset.testid === "tab-session-chat-messages" ? 600 : 0; },
  });
  class StubResizeObserver {
    observe(): void {}
    unobserve(): void {}
    disconnect(): void {}
  }
  Object.defineProperty(globalThis, "ResizeObserver", { configurable: true, value: StubResizeObserver });
  return window;
}

function tabBase(id: string): Tab {
  return {
    id, pty_id: null, tab_type: "Terminal", title: id, cwd: null,
    backing: { kind: "chat_backend", plugin_id: "lmstudio", session_id: "sess-1" },
  } as Tab;
}

function originTab(id: string): Tab {
  return { id, pty_id: null, tab_type: "Terminal", title: id, cwd: null, agent_type: "claude" } as Tab;
}

function muxActions(): MultiplexerActionsContextType {
  const noop = () => {};
  const noopAsync = async () => undefined;
  return new Proxy({}, {
    get: (_t, prop) => {
      if (prop === "spawnAgentTab") return async () => null;
      if (prop === "createOrchestrationPair") return async () => { throw new Error("not implemented"); };
      if (prop === "createWorkspace" || prop === "createTab") return noopAsync;
      if (prop === "selectTabByTab") return noopAsync;
      return noop;
    },
  }) as MultiplexerActionsContextType;
}

function muxStateFor(tabs: Record<string, Tab>): ExtendedState {
  return {
    workspaces: [], tabs, teams: [], active_workspace_id: null,
    focused_tab_id: Object.keys(tabs)[0] ?? null,
    notifications: [], tabStatuses: {}, toasts: [], activeTabIds: {},
    generation: 1,
    pendingCloses: [],
  };
}

function assistant(id: string, content: string, seq: number) {
  return { id, type: "assistant", content, timestamp: String(1_700_000_000_000 + seq * 1000), seq, provider: "test" };
}

// User message carrying a codeterm tab marker → enriches to origin.kind === "tab".
function originReport(id: string, shortId: string, seq: number) {
  return {
    id,
    type: "user",
    content: `<domios from="tab" label="Worker" tab="${shortId}">report body ${seq}</domios>`,
    timestamp: String(1_700_000_000_000 + seq * 1000),
    seq,
    provider: "test",
  };
}

async function main() {
  const testWindow = setupDom();
  const { __setInvokeForTest, __setListenForTest } = await import("../../lib/bridge");
  const { TabSessionChat } = await import("./TabSessionChat");
  const {
    addChatSend, ensureChatSendTabHydrated, forgetChatSendsInMemory, observeDeliveryStatus,
    __setChatSendRepositoryForTest,
  } = await import("./chatSendStore");
  const { MemoryChatSendRepository } = await import("./chatSendRepository.testSupport");

  const STABLE_COUNT = 6;
  const CHUNKS = 12;
  const tabId = "chat-tab";
  const originShort = "wrkr0001";
  const originFull = "wrkr0001-full-tab-id";

  const initial: ReturnType<typeof assistant>[] = [];
  for (let i = 0; i < STABLE_COUNT; i++) {
    initial.push(originReport(`m${i}`, originShort, i) as never);
  }
  initial.push(assistant("tail", "streaming", STABLE_COUNT));

  let chatStartOverride: (() => unknown) | null = null;
  let chatSendStatusesOverride: unknown[] = [];

  __setInvokeForTest((async (cmd: string) => {
    if (cmd === "chat_start") {
      return chatStartOverride
        ? chatStartOverride()
        : { text: "", offset: 0, messages: initial, pending: false, source_session_id: null, session_binding_nonce: null };
    }
    if (cmd === "chat_send_status") return chatSendStatusesOverride;
    if (cmd === "delayed_send_list") return [];
    if (cmd === "list_agent_providers") return [];
    return {};
  }) as never);
  __setListenForTest((async (event: string, cb: Handler) => {
    let set = listeners.get(event);
    if (!set) { set = new Set(); listeners.set(event, set); }
    set.add(cb);
    return () => { set?.delete(cb); };
  }) as never);

  test("streamed tail deltas do not re-render unchanged origin bubbles", async () => {
    (window as { __bubbleRenderCounts?: Record<string, number> }).__bubbleRenderCounts = {};
    const host = document.createElement("div");
    document.body.appendChild(host);
    const root = createRoot(host);
    const store = createMuxStateStore(() => muxStateFor({
      [tabId]: tabBase(tabId),
      [originFull]: originTab(originFull),
    }));
    await act(async () => {
      root.render(
        <MuxStateStoreContext.Provider value={store}>
          <MultiplexerActionsContext.Provider value={muxActions()}>
            <TabSessionChat tabId={tabId} chatBackend isVisible />
          </MultiplexerActionsContext.Provider>
        </MuxStateStoreContext.Provider>,
      );
    });
    await act(async () => { await Promise.resolve(); });

    const counts = () => (window as { __bubbleRenderCounts?: Record<string, number> }).__bubbleRenderCounts ?? {};
    const stableId = host.querySelector('[data-message-role="user"]')?.getAttribute("data-message-id");
    assert(stableId, "origin bubble row mounted");
    const baseStable = counts()[stableId] ?? 0;
    assert(baseStable > 0, "origin bubbles mounted");
    // The avatar (proof the origin actually resolved to a provider) must render.
    const avatarMounted = host.querySelectorAll("svg").length > 0 || host.textContent?.includes("report body 0");
    assert(avatarMounted, "origin report rendered");

    for (let c = 0; c < CHUNKS; c++) {
      await act(async () => {
        emit(`tab-chat-msg-${tabId}`, { tab_id: tabId, source_session_id: null, session_binding_nonce: null, messages: [assistant("tail", "streaming".padEnd(10 + c, "x"), STABLE_COUNT)] });
        await Promise.resolve();
      });
    }

    const stableRenders = (counts()[stableId] ?? 0) - baseStable;
    const tailRenders = counts()["tail"] ?? 0;
    // The tail actually streamed (proves the harness drove updates)...
    assert(tailRenders >= 3, `tail should re-render on streaming (got ${tailRenders})`);
    // ...but unchanged origin rows must stay flat, not grow ~1 per chunk.
    assert(stableRenders <= 2, `unchanged origin bubble re-rendered ${stableRenders}x over ${CHUNKS} chunks (fix regressed: inline onOriginClick/originAvatar churn)`);

    console.log(`  origin-stability: stable +${stableRenders} over ${CHUNKS} chunks, tail ${tailRenders}`);
    await act(async () => root.unmount());
  });

  test("keeps one collapsed operation at its source position through idle and completion", async () => {
    const liveTabId = "live-operation-tab";
    const command = "domios plugin browser goto http://127.0.0.1:14997/;\ndomios plugin browser screenshot evidence/chat-running-dark.png";
    const operation = { id: "live-tool", type: "tool_call", content: "", toolId: "live-tool", toolKind: "command", toolInput: { cmd: command }, toolSummary: "browser fixture", toolResult: "opaque-live-output", toolStatus: "running", toolStartedAtMs: Date.now() - 3000, seq: 1 };
    initial.splice(0, initial.length, operation as never, assistant("after-tool", "Inspecting the page", 2));
    let state: ExtendedState = { ...muxStateFor({ [liveTabId]: { ...tabBase(liveTabId), agent_state: { state: "working" } } as Tab }), tabStatuses: { [liveTabId]: "Working" } };
    const store = createMuxStateStore(() => state);
    const host = document.body.appendChild(document.createElement("div"));
    const root = createRoot(host);
    await act(async () => root.render(
      <ChatBlockHostProvider value={{ renderMenu: ({ items }) => <>{items.map(item => <button key={item.label} onClick={item.onSelect}>{item.label}</button>)}</> }}><MuxStateStoreContext.Provider value={store}>
        <MultiplexerActionsContext.Provider value={muxActions()}>
          <TabSessionChat tabId={liveTabId} chatBackend isVisible />
        </MultiplexerActionsContext.Provider>
      </MuxStateStoreContext.Provider></ChatBlockHostProvider>,
    ));
    const log = host.querySelector('[role="log"]');
    const live = log?.querySelector('[data-testid="chat-live-operation"]');
    assert(live?.textContent?.includes("browser"), "live item names the command");
    assert(live?.textContent?.includes("evidence/chat-running-dark.png"), "compact tree names both command segments");
    assert(!live?.textContent?.includes("opaque-live-output"), "live command output starts collapsed");
    const later = log?.querySelector('[data-message-id="after-tool"]');
    assert(live && later && live.compareDocumentPosition(later) & Node.DOCUMENT_POSITION_FOLLOWING, "the live operation stays before later output");
    assert(log?.querySelectorAll('[data-testid="chat-live-operation"]').length === 1, "one live item");
    assert(log?.querySelectorAll('[data-chat-window-row="live-tool"] [data-testid="chat-command-group"]').length === 1, "one normal tool card owns both live and finished presentation");
    assert(live?.querySelector('[role="timer"]')?.textContent?.includes("Running"), "elapsed progress renders");
    const expand = live?.querySelector<HTMLButtonElement>('[data-testid="chat-command-group-toggle"]');
    assert(expand?.getAttribute("aria-expanded") === "false", "live command group starts collapsed");
    await act(async () => expand?.click());
    assert(expand?.getAttribute("aria-expanded") === "true", "full operation can be expanded");
    await act(async () => live?.querySelector<HTMLElement>('[data-testid="chat-message"]')?.focus());
    const actions = live?.querySelector<HTMLButtonElement>('button[aria-label="Message actions"]');
    assert(actions, "the operation shares the row's message actions");
    await act(async () => actions.click());
    const details = document.querySelector<HTMLButtonElement>('[role="menuitem"][aria-label="Details"]');
    assert(details, "live operation offers output details");
    await act(async () => details.click());
    assert(live?.textContent?.includes("opaque-live-output"), "expanded operation reveals its current output");
    await act(async () => {
      state = { ...state, tabStatuses: { [liveTabId]: "Idle" }, tabs: { [liveTabId]: { ...state.tabs[liveTabId], agent_state: { state: "working" } } as Tab } };
      store.notify();
    });
    assert(!log?.querySelector('[data-testid="chat-live-operation"]'), "idle hides stale running activity");
    await act(async () => {
      state = { ...state, tabStatuses: { [liveTabId]: "Working" }, tabs: { [liveTabId]: { ...state.tabs[liveTabId], agent_state: { state: "working" } } as Tab } };
      store.notify();
      emit(`tab-chat-msg-${liveTabId}`, { tab_id: liveTabId, source_session_id: null, session_binding_nonce: null, messages: [{ ...operation, toolStatus: "completed", toolCompletedAtMs: Date.now() }] });
    });
    assert(!log?.querySelector('[data-testid="chat-live-operation"]'), "completed operation clears its live marker");
    assert(log?.querySelector('[data-chat-window-row="live-tool"]') === live, "settlement preserves the same operation row node");
    assert(log?.querySelector('[data-chat-window-row="live-tool"] [data-testid="chat-command-group"]'), "completed operation stays in its normal row");
    await act(async () => root.unmount());
    host.remove();
  });

  test("authenticated incoming routed send stays at its authoritative position through work, tool patch, and reentry", async () => {
    const ritchieTabId = "ritchie-tab";
    const sessionId = "sess-ritchie";
    const bindingNonce = "nonce-ritchie";
    const dumasShort = "duma0001";
    const dumasFull = "duma0001-full-tab-id";
    const baseAt = 1_700_000_000_000;
    const dumasBody = "opaque dumas guidance";
    const dumasWire = `<domios from="tab" label="Dumas" tab="${dumasShort}">${dumasBody}</domios>`;
    const dumasSegments = [{
      type: "frame" as const, kind: "tab" as const, from: "tab",
      attrs: { from: "tab", label: "Dumas", tab: dumasShort }, body: dumasBody, tip: null,
    }];
    const startUser = { id: "start-user", type: "user", content: "initial task", timestamp: String(baseAt), seq: 0, provider: "test", sourceSessionId: sessionId };
    const earlyWork = { id: "early-work", type: "assistant", content: "investigating", timestamp: String(baseAt + 1000), seq: 1, provider: "test", sourceSessionId: sessionId };
    const dumasEcho = { id: "provider-dumas", type: "user", content: dumasWire, correlationId: "dumas-token", timestamp: String(baseAt + 2000), seq: 2, provider: "test", sourceSessionId: sessionId };
    const laterAssistant = { id: "later-assistant", type: "assistant", content: "new progress", timestamp: String(baseAt + 3000), seq: 3, provider: "test", sourceSessionId: sessionId };
    const laterTool = {
      id: "later-tool", type: "tool_call", content: "", toolId: "later-tool", toolKind: "command",
      toolInput: { cmd: "git status" }, toolSummary: "check status", toolResult: "step 1", toolStatus: "running",
      toolStartedAtMs: baseAt + 4000, timestamp: String(baseAt + 4000), seq: 4, provider: "test", sourceSessionId: sessionId,
    };
    const patchedTool = { ...laterTool, toolResult: "step 1\nstep 2", toolStatus: "completed", toolCompletedAtMs: baseAt + 5000 };
    const dumasWrittenStatus = {
      tab_id: ritchieTabId, send_id: "dumas-send", correlationId: "dumas-token", state: "written" as const,
      sequence: 1, ledger_id: "ledger", retained_from: 1, error: null,
      system_send: { content: dumasWire, sent_at_ms: baseAt + 2000, segments: dumasSegments, segments_version: 2 },
    };
    const dumasDeliveredStatus = { ...dumasWrittenStatus, state: "delivered" as const, sequence: 2 };
    const ritchieTab = {
      id: ritchieTabId, pty_id: null, tab_type: "Terminal", title: "Ritchie", cwd: null,
      agent_type: "claude", agent_session_id: sessionId, session_binding_nonce: bindingNonce,
      agent_state: { state: "working" },
      backing: { kind: "pty" },
    } as Tab;
    const state: ExtendedState = {
      ...muxStateFor({ [ritchieTabId]: ritchieTab, [dumasFull]: originTab(dumasFull) }),
      tabStatuses: { [ritchieTabId]: "Working" },
    };
    const store = createMuxStateStore(() => state);
    const repository = new MemoryChatSendRepository();
    __setChatSendRepositoryForTest(repository);
    const host = document.body.appendChild(document.createElement("div"));
    let root = createRoot(host);
    const renderChat = () => (
      <MuxStateStoreContext.Provider value={store}>
        <MultiplexerActionsContext.Provider value={muxActions()}>
          <TabSessionChat tabId={ritchieTabId} chatBackend isVisible />
        </MultiplexerActionsContext.Provider>
      </MuxStateStoreContext.Provider>
    );

    try {
      chatStartOverride = () => ({
        text: "", offset: 0, messages: [startUser, earlyWork],
        pending: false, source_session_id: sessionId, session_binding_nonce: bindingNonce,
      });
      chatSendStatusesOverride = [];
      await act(async () => root.render(renderChat()));
      await act(async () => {
        await ensureChatSendTabHydrated(ritchieTabId);
        await Promise.resolve();
      });

      await act(async () => {
        await observeDeliveryStatus(dumasWrittenStatus);
        addChatSend(ritchieTabId, { id: "outbound-send", correlationId: "outbound-token", content: "owner follow-up", sentAtMs: baseAt + 2500 });
        await Promise.resolve();
      });

      await act(async () => {
        emit(`tab-chat-msg-${ritchieTabId}`, {
          tab_id: ritchieTabId, source_session_id: sessionId, session_binding_nonce: bindingNonce,
          messages: [laterAssistant, laterTool],
        });
        await Promise.resolve();
      });

      const log = host.querySelector('[role="log"]');
      const earlyEl = log?.querySelector<HTMLElement>('[data-message-id="early-work"]');
      const dumasEl = log?.querySelector<HTMLElement>('[data-message-id="dumas-send"]');
      const laterAssistantEl = log?.querySelector<HTMLElement>('[data-message-id="later-assistant"]');
      const laterToolEl = log?.querySelector<HTMLElement>('[data-chat-window-row="later-tool"]');
      const outboundEl = log?.querySelector<HTMLElement>('[data-message-id="outbound-send"]');
      assert(log?.querySelectorAll('[data-message-id="dumas-send"]').length === 1, "one Dumas DOM row");
      assert(dumasEl?.textContent?.includes("Dumas"), "Dumas sender label rendered");
      assert(dumasEl?.textContent?.includes(dumasBody), "Dumas body rendered cleanly");
      assert(!dumasEl?.textContent?.includes("<domios"), "Dumas row is not raw envelope XML");
      assert(earlyEl && dumasEl && Boolean(earlyEl.compareDocumentPosition(dumasEl) & Node.DOCUMENT_POSITION_FOLLOWING), "Dumas follows earlier work");
      assert(dumasEl && laterAssistantEl && Boolean(dumasEl.compareDocumentPosition(laterAssistantEl) & Node.DOCUMENT_POSITION_FOLLOWING), "later assistant renders below Dumas");
      assert(laterAssistantEl && laterToolEl && Boolean(laterAssistantEl.compareDocumentPosition(laterToolEl) & Node.DOCUMENT_POSITION_FOLLOWING), "later tool renders below later assistant");
      assert(laterToolEl && outboundEl && Boolean(laterToolEl.compareDocumentPosition(outboundEl) & Node.DOCUMENT_POSITION_FOLLOWING), "genuine outbound send stays held at tail");

      await act(async () => {
        emit(`tab-chat-msg-${ritchieTabId}`, {
          tab_id: ritchieTabId, source_session_id: sessionId, session_binding_nonce: bindingNonce,
          messages: [patchedTool],
        });
        await Promise.resolve();
      });
      assert(log?.querySelector('[data-message-id="dumas-send"]') === dumasEl, "tool patch preserves the same Dumas DOM node");
      assert(log?.querySelectorAll('[data-message-id="dumas-send"]').length === 1, "no duplicate Dumas row after tool patch");
      assert(dumasEl && laterToolEl && Boolean(dumasEl.compareDocumentPosition(laterToolEl) & Node.DOCUMENT_POSITION_FOLLOWING), "Dumas stays above patched tool row");

      chatStartOverride = () => ({
        text: "", offset: 0, messages: [startUser, earlyWork, dumasEcho, laterAssistant, patchedTool],
        pending: false, source_session_id: sessionId, session_binding_nonce: bindingNonce,
      });
      chatSendStatusesOverride = [dumasDeliveredStatus];
      await act(async () => root.unmount());
      root = createRoot(host);
      await act(async () => root.render(renderChat()));
      await act(async () => {
        await Promise.resolve();
        await Promise.resolve();
      });

      const reentryLog = host.querySelector('[role="log"]');
      const reentryEarly = reentryLog?.querySelector<HTMLElement>('[data-message-id="early-work"]');
      const reentryDumas = reentryLog?.querySelector<HTMLElement>('[data-message-id="dumas-send"]');
      const reentryAssistant = reentryLog?.querySelector<HTMLElement>('[data-message-id="later-assistant"]');
      const reentryTool = reentryLog?.querySelector<HTMLElement>('[data-chat-window-row="later-tool"]');
      const reentryOutbound = reentryLog?.querySelector<HTMLElement>('[data-message-id="outbound-send"]');
      assert(reentryLog?.querySelectorAll('[data-message-id="dumas-send"]').length === 1, "one Dumas DOM row after reentry");
      assert(reentryDumas?.textContent?.includes("Dumas"), "Dumas sender preserved after reentry");
      assert(reentryDumas?.textContent?.includes(dumasBody), "Dumas body preserved after reentry");
      assert(!reentryDumas?.textContent?.includes("<domios"), "Dumas row stays clean after reentry");
      assert(reentryEarly && reentryDumas && Boolean(reentryEarly.compareDocumentPosition(reentryDumas) & Node.DOCUMENT_POSITION_FOLLOWING), "Dumas follows earlier work after reentry");
      assert(reentryDumas && reentryAssistant && Boolean(reentryDumas.compareDocumentPosition(reentryAssistant) & Node.DOCUMENT_POSITION_FOLLOWING), "later assistant stays below Dumas after reentry");
      assert(reentryAssistant && reentryTool && Boolean(reentryAssistant.compareDocumentPosition(reentryTool) & Node.DOCUMENT_POSITION_FOLLOWING), "later tool stays below later assistant after reentry");
      assert(reentryTool && reentryOutbound && Boolean(reentryTool.compareDocumentPosition(reentryOutbound) & Node.DOCUMENT_POSITION_FOLLOWING), "genuine outbound hold stays at tail after reentry");
    } finally {
      chatStartOverride = null;
      chatSendStatusesOverride = [];
      await act(async () => root.unmount());
      host.remove();
      __setChatSendRepositoryForTest(null);
      forgetChatSendsInMemory();
    }
  });

  let failed = 0;
  for (const [name, fn] of tests) {
    try { await fn(); console.log(`PASS ${name}`); }
    catch (error) { failed += 1; console.error(`FAIL ${name}`, error); }
  }
  if (failed > 0) throw new Error(`${failed} test(s) failed`);
  __setInvokeForTest(null);
  __setListenForTest(null);
  listeners.clear();
  await testWindow.happyDOM.abort();
  testWindow.close();
}

main().then(() => process.exit(0)).catch((error) => {
  console.error(error);
  process.exit(1);
});
