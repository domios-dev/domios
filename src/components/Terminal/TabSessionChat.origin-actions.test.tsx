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

function actionsWith(selectTabByTab: (workspaceId: string, tabId: string) => void): MultiplexerActionsContextType {
  const noop = () => {};
  const noopAsync = async () => undefined;
  return new Proxy({}, {
    get: (_t, prop) => {
      if (prop === "selectTabByTab") return async (workspaceId: string, tabId: string) => selectTabByTab(workspaceId, tabId);
      if (prop === "spawnAgentTab") return async () => null;
      if (prop === "createOrchestrationPair") return async () => { throw new Error("not implemented"); };
      if (prop === "createWorkspace" || prop === "createTab") return noopAsync;
      return noop;
    },
  }) as MultiplexerActionsContextType;
}

function muxStateFor(tabs: Record<string, Tab>): ExtendedState {
  return { workspaces: [
    { id: "chat-workspace", name: "Chat", cwd: null, tabs: ["chat-tab"], active_tab_id: "chat-tab" },
    { id: "sender-workspace", name: "Sender", cwd: null, tabs: ["wrkr0001-full-tab-id"], active_tab_id: "wrkr0001-full-tab-id" },
  ], tabs, teams: [], active_workspace_id: "chat-workspace", focused_tab_id: Object.keys(tabs)[0] ?? null, notifications: [], tabStatuses: {}, toasts: [], activeTabIds: {}, generation: 1, pendingCloses: [] } as ExtendedState;
}
function originReport(id: string, shortId: string, seq: number) {
  return { id, type: "user", content: `<domios from="tab" label="Worker" tab="${shortId}">report body ${seq}</domios>`, timestamp: String(1_700_000_000_000 + seq * 1000), seq, provider: "test",
    segments: [{ type: "frame", kind: "tab", from: "tab", attrs: { from: "tab", label: "Worker", tab: shortId }, body: `report body ${seq}`, tip: null }],
  };
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

  test("routed Show Tab selects the sender workspace by click and Enter", async () => {
    const calls: string[] = [];
    const select = (workspaceId: string, id: string) => { calls.push(`${workspaceId}:${id}`); };

    const host = document.createElement("div");
    document.body.appendChild(host);
    const root = createRoot(host);
    const store = createMuxStateStore(() => muxStateFor({ [tabId]: tabBase(tabId), [originFull]: originTab(originFull) }));

    const renderWith = () =>
      root.render(
        <MuxStateStoreContext.Provider value={store}>
          <MultiplexerActionsContext.Provider value={actionsWith(select)}>
            <TabSessionChat tabId={tabId} chatBackend isVisible />
          </MultiplexerActionsContext.Provider>
        </MuxStateStoreContext.Provider>,
      );

    await act(async () => { renderWith(); });
    await act(async () => { await Promise.resolve(); });
    const button = host.querySelector<HTMLButtonElement>('button[data-testid="inbound-show-tab"]');
    assert(button, "origin 'Show Tab' action button present");
    await act(async () => { button.click(); });
    assert(calls[0] === `sender-workspace:${originFull}`, `click selects the sender workspace and tab (got ${JSON.stringify(calls)})`);

    calls.length = 0;
    await act(async () => {
      button.focus();
      button.dispatchEvent(new testWindow.KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
      button.click();
      button.dispatchEvent(new testWindow.KeyboardEvent("keyup", { key: "Enter", bubbles: true }));
    });
    assert(calls[0] === `sender-workspace:${originFull}`, `Enter selects the sender workspace and tab (got ${JSON.stringify(calls)})`);
    assert(calls.length === 1, `activation selects once (got ${JSON.stringify(calls)})`);
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
