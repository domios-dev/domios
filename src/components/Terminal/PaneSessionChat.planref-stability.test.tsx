// Run: npx tsx src/components/Terminal/PaneSessionChat.planref-stability.test.tsx
//
// Render-count oracle for the composer-freeze fix. A long chat streams many
// tail-delta chunks (as codex does per token batch). Every chunk re-renders
// PaneSessionChat; the fix keeps visiblePlanRefs (and thus resolvePlanRef)
// referentially stable when the ref-set is unchanged, so memoized MessageBubble
// rows for UNCHANGED messages must NOT re-render per chunk. Before the fix each
// chunk churned resolvePlanRef and re-rendered the whole visible list.

import { act } from "react";
import { Window } from "happy-dom";
import { createRoot } from "react-dom/client";
import type { Pane } from "../../lib/types";
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
}

function paneBase(id: string): Pane {
  return {
    id, pty_id: null, pane_type: "Terminal", title: id, cwd: null,
    backing: { kind: "chat_backend", plugin_id: "lmstudio", session_id: "sess-1" },
  } as Pane;
}

function muxActions(): MultiplexerActionsContextType {
  const noop = () => {};
  const noopAsync = async () => undefined;
  return new Proxy({}, {
    get: (_t, prop) => {
      if (prop === "spawnAgentTab") return async () => null;
      if (prop === "createOrchestrationPair") return async () => { throw new Error("not implemented"); };
      if (prop === "createWorkspace" || prop === "createTab") return noopAsync;
      if (prop === "selectTabByPane") return noopAsync;
      return noop;
    },
  }) as MultiplexerActionsContextType;
}

function muxStateFor(panes: Record<string, Pane>): ExtendedState {
  return {
    workspaces: [], panes, teams: [], active_workspace_id: null,
    focused_pane_id: Object.keys(panes)[0] ?? null,
    notifications: [], paneStatuses: {}, toasts: [], activeTabIds: {},
    generation: 1,
  };
}

function assistant(id: string, content: string, seq: number) {
  return { id, type: "assistant", content, timestamp: String(1_700_000_000_000 + seq * 1000), seq, provider: "test" };
}

async function main() {
  setupDom();
  const { __setInvokeForTest, __setListenForTest } = await import("../../lib/bridge");
  const { PaneSessionChat } = await import("./PaneSessionChat");

  const STABLE_COUNT = 6;
  const CHUNKS = 12;
  const paneId = "chat-pane";
  const initial: ReturnType<typeof assistant>[] = [];
  for (let i = 0; i < STABLE_COUNT; i++) initial.push(assistant(`m${i}`, `stable message ${i}`, i));
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

  test("streamed tail deltas do not re-render unchanged bubbles", async () => {
    (window as { __bubbleRenderCounts?: Record<string, number> }).__bubbleRenderCounts = {};
    const host = document.createElement("div");
    document.body.appendChild(host);
    const root = createRoot(host);
    const store = createMuxStateStore(() => muxStateFor({ [paneId]: paneBase(paneId) }));
    await act(async () => {
      root.render(
        <MuxStateStoreContext.Provider value={store}>
          <MultiplexerActionsContext.Provider value={muxActions()}>
            <PaneSessionChat paneId={paneId} chatBackend isVisible />
          </MultiplexerActionsContext.Provider>
        </MuxStateStoreContext.Provider>,
      );
    });
    await act(async () => { await Promise.resolve(); });

    const counts = () => (window as { __bubbleRenderCounts?: Record<string, number> }).__bubbleRenderCounts ?? {};
    const baseStable = counts()["m0"] ?? 0;
    assert(baseStable > 0, "stable bubbles mounted");

    for (let c = 0; c < CHUNKS; c++) {
      await act(async () => {
        emit(`pane-chat-msg-${paneId}`, { pane_id: paneId, source_session_id: null, session_binding_nonce: null, messages: [assistant("tail", "streaming".padEnd(10 + c, "x"), STABLE_COUNT)] });
        await Promise.resolve();
      });
    }

    const stableRenders = (counts()["m0"] ?? 0) - baseStable;
    const tailRenders = counts()["tail"] ?? 0;
    // The tail actually streamed (proves the harness drove updates)...
    assert(tailRenders >= 3, `tail should re-render on streaming (got ${tailRenders})`);
    // ...but unchanged rows must stay flat, not grow ~1 per chunk.
    assert(stableRenders <= 2, `unchanged bubble re-rendered ${stableRenders}x over ${CHUNKS} chunks (fix regressed: resolvePlanRef churn)`);

    await act(async () => root.unmount());
  });

  let failed = 0;
  for (const [name, fn] of tests) {
    try { await fn(); console.log(`PASS ${name}`); }
    catch (error) { failed += 1; console.error(`FAIL ${name}`, error); }
  }
  if (failed > 0) throw new Error(`${failed} test(s) failed`);
}

void main();
