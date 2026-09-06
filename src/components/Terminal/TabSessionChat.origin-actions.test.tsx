// The origin cache must invalidate when `setFocusedTab` changes, or a stale closure routes Show Tab to the wrong handler.

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

function setupDom() {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const window = new Window({ url: "http://localhost/" });
  for (const key of ["window", "document", "navigator", "HTMLElement", "HTMLTextAreaElement", "SVGElement", "Element", "Node", "Event", "CustomEvent", "MouseEvent"] as const) {
    Object.defineProperty(globalThis, key, { value: (window as never)[key], configurable: true });
  }
  Object.defineProperty(globalThis, "requestAnimationFrame", { configurable: true, value: (cb: FrameRequestCallback) => { cb(0); return 0; } });
  Object.defineProperty(globalThis, "cancelAnimationFrame", { configurable: true, value: () => {} });
  class StubResizeObserver { observe(): void {} unobserve(): void {} disconnect(): void {} }
  Object.defineProperty(globalThis, "ResizeObserver", { configurable: true, value: StubResizeObserver });
  return window;
}

function tabBase(id: string): Tab {
  return { id, pty_id: null, tab_type: "Terminal", title: id, cwd: null, backing: { kind: "chat_backend", plugin_id: "lmstudio", session_id: "sess-1" } } as Tab;
}
function originTab(id: string): Tab { return { id, pty_id: null, tab_type: "Terminal", title: id, cwd: null, agent_type: "claude" } as Tab; }

function actionsWith(setFocusedTab: (tabId: string) => void): MultiplexerActionsContextType {
  const noop = () => {};
  const noopAsync = async () => undefined;
  return new Proxy({}, {
    get: (_t, prop) => {
      if (prop === "setFocusedTab") return setFocusedTab;
      if (prop === "spawnAgentTab") return async () => null;
      if (prop === "createOrchestrationPair") return async () => { throw new Error("not implemented"); };
      if (prop === "createWorkspace" || prop === "createTab" || prop === "selectTabByTab") return noopAsync;
      return noop;
    },
  }) as MultiplexerActionsContextType;
}

function muxStateFor(tabs: Record<string, Tab>): ExtendedState {
  return { workspaces: [], tabs, teams: [], active_workspace_id: null, focused_tab_id: Object.keys(tabs)[0] ?? null, notifications: [], tabStatuses: {}, toasts: [], activeTabIds: {}, generation: 1, pendingCloses: [] };
}
function originReport(id: string, shortId: string, seq: number) {
  return { id, type: "user", content: `-=-codeterm:tab label=Worker tab=${shortId}-=-report body ${seq}`, timestamp: String(1_700_000_000_000 + seq * 1000), seq, provider: "test" };
}

async function main() {
  const testWindow = setupDom();
  const { __setInvokeForTest, __setListenForTest } = await import("../../lib/bridge");
  const { TabSessionChat } = await import("./TabSessionChat");

  const tabId = "chat-tab";
  const originShort = "wrkr0001";
  const originFull = "wrkr0001-full-tab-id";
  const initial = [originReport("m0", originShort, 0)];

  __setInvokeForTest((async (cmd: string) => {
    if (cmd === "chat_start") return { text: "", offset: 0, messages: initial, pending: false, source_session_id: null, session_binding_nonce: null };
    if (cmd === "delayed_send_list" || cmd === "list_agent_providers") return [];
    return {};
  }) as never);
  __setListenForTest((async (event: string, cb: Handler) => {
    let set = listeners.get(event); if (!set) { set = new Set(); listeners.set(event, set); }
    set.add(cb); return () => { set?.delete(cb); };
  }) as never);

  test("origin action routes to the current setFocusedTab after its identity changes", async () => {
    const calls: string[] = [];
    const spyA = (id: string) => { calls.push(`A:${id}`); };
    const spyB = (id: string) => { calls.push(`B:${id}`); };

    const host = document.createElement("div");
    document.body.appendChild(host);
    const root = createRoot(host);
    const store = createMuxStateStore(() => muxStateFor({ [tabId]: tabBase(tabId), [originFull]: originTab(originFull) }));

    const renderWith = (actions: MultiplexerActionsContextType) =>
      root.render(
        <MuxStateStoreContext.Provider value={store}>
          <MultiplexerActionsContext.Provider value={actions}>
            <TabSessionChat tabId={tabId} chatBackend isVisible />
          </MultiplexerActionsContext.Provider>
        </MuxStateStoreContext.Provider>,
      );

    await act(async () => { renderWith(actionsWith(spyA)); });
    await act(async () => { await Promise.resolve(); });

    // Re-render with a FRESH setFocusedTab identity; tab/provider key is unchanged.
    await act(async () => { renderWith(actionsWith(spyB)); });
    await act(async () => { await Promise.resolve(); });

    const button = host.querySelector<HTMLButtonElement>('button[title="Show Tab"]');
    assert(button, "origin 'Show Tab' action button present");
    await act(async () => {
      button!.dispatchEvent(new (globalThis as { MouseEvent: typeof MouseEvent }).MouseEvent("click", { bubbles: true }));
    });

    assert(calls.length === 1, `exactly one focus call (got ${JSON.stringify(calls)})`);
    assert(calls[0] === `B:${originFull}`, `click must route to CURRENT setFocusedTab (got ${JSON.stringify(calls)})`);
    console.log(`  origin-actions: routed ${calls[0]}`);
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
