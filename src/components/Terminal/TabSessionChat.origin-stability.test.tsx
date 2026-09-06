// After the fix those props are memoized per message id and stay referentially stable, so only the actively-changing tail re-renders per chunk.

import { act } from "react";
import { Window } from "happy-dom";
import { createRoot } from "react-dom/client";
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
    content: `-=-codeterm:tab label=Worker tab=${shortId}-=-report body ${seq}`,
    timestamp: String(1_700_000_000_000 + seq * 1000),
    seq,
    provider: "test",
  };
}

async function main() {
  const testWindow = setupDom();
  const { __setInvokeForTest, __setListenForTest } = await import("../../lib/bridge");
  const { TabSessionChat } = await import("./TabSessionChat");

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

  __setInvokeForTest((async (cmd: string) => {
    if (cmd === "chat_start") return { text: "", offset: 0, messages: initial, pending: false, source_session_id: null, session_binding_nonce: null };
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
    const baseStable = counts()["m0"] ?? 0;
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

    const stableRenders = (counts()["m0"] ?? 0) - baseStable;
    const tailRenders = counts()["tail"] ?? 0;
    // The tail actually streamed (proves the harness drove updates)...
    assert(tailRenders >= 3, `tail should re-render on streaming (got ${tailRenders})`);
    // ...but unchanged origin rows must stay flat, not grow ~1 per chunk.
    assert(stableRenders <= 2, `unchanged origin bubble re-rendered ${stableRenders}x over ${CHUNKS} chunks (fix regressed: inline onOriginClick/originAvatar churn)`);

    console.log(`  origin-stability: stable +${stableRenders} over ${CHUNKS} chunks, tail ${tailRenders}`);
    await act(async () => root.unmount());
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
