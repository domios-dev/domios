// Run: npx tsx src/components/Terminal/TabSessionChat.stop.test.tsx

import { act } from "react";
import { Window } from "happy-dom";
import { createRoot } from "react-dom/client";
import type { Tab } from "../../lib/types";
import { MultiplexerActionsContext, MuxStateStoreContext, type MultiplexerActionsContextType } from "../../store/multiplexer/context";
import { createMuxStateStore } from "../../store/multiplexer/selectorStore";
import type { ExtendedState } from "../../store/multiplexer/tree";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function setupDom() {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const window = new Window({ url: "http://localhost/" });
  for (const [key, value] of Object.entries({
    window, document: window.document, navigator: window.navigator, HTMLElement: window.HTMLElement,
    HTMLTextAreaElement: window.HTMLTextAreaElement, SVGElement: window.SVGElement, Element: window.Element,
    Node: window.Node, Event: window.Event, CustomEvent: window.CustomEvent,
    requestAnimationFrame: (cb: FrameRequestCallback) => { cb(0); return 0; }, cancelAnimationFrame: () => {},
    ResizeObserver: class { observe(): void {} unobserve(): void {} disconnect(): void {} },
  })) Object.defineProperty(globalThis, key, { value, configurable: true });
  return window;
}

const actions = new Proxy({}, { get: () => () => undefined }) as MultiplexerActionsContextType;

async function main() {
  const testWindow = setupDom();
  const { __setInvokeForTest, __setListenForTest } = await import("../../lib/bridge");
  const { TabSessionChat } = await import("./TabSessionChat");
  const interrupts: unknown[] = [];
  __setInvokeForTest((async (cmd: string, args?: unknown) => {
    if (cmd === "interrupt_agent") interrupts.push(args);
    if (cmd === "chat_start") return { source_session_id: "thread-1", session_binding_nonce: null, subscription_id: "s", text: "", offset: 0, messages: [] };
    if (cmd === "delayed_send_list") return [];
    return {};
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
  }) as any);
  __setListenForTest(async () => () => {});

  const tab = {
    id: "runtime-tab", pty_id: null, tab_type: "Terminal", title: "Codex", cwd: null, agent_type: "codex",
    agent_session_id: "thread-1", boot_stage: "agent_ready",
    backing: { kind: "chat_backend", plugin_id: "codex", session_id: "thread-1" },
    agent_state: { state: "working", status_line: "Working" },
  } as Tab;
  const state = {
    workspaces: [], tabs: { [tab.id]: tab }, teams: [], active_workspace_id: null, focused_tab_id: tab.id,
    notifications: [], tabStatuses: {}, toasts: [], activeTabIds: {}, generation: 1, pendingCloses: [],
  } as unknown as ExtendedState;
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(
      <MuxStateStoreContext.Provider value={createMuxStateStore(() => state)}>
        <MultiplexerActionsContext.Provider value={actions}>
          <TabSessionChat tabId={tab.id} chatBackend isVisible />
        </MultiplexerActionsContext.Provider>
      </MuxStateStoreContext.Provider>,
    );
  });
  await act(async () => { await Promise.resolve(); });

  const stops = [...host.querySelectorAll("button")].filter((button) =>
    button.getAttribute("aria-label") === "Stop the agent" && !button.disabled && button.getAttribute("aria-hidden") !== "true");
  assert(stops.length === 1, `a working chat_backend tab renders one accessible Stop button, got ${stops.length}`);
  await act(async () => { stops[0].click(); });
  assert(interrupts.length === 1, `Stop interrupts once, got ${interrupts.length}`);
  assert(JSON.stringify(interrupts[0]) === JSON.stringify({ tabId: tab.id }), `Stop routes the runtime turn by tab, got ${JSON.stringify(interrupts[0])}`);
  console.log("PASS working chat_backend tab renders an accessible Stop that interrupts once");

  await act(async () => root.unmount());
  host.remove();
  __setInvokeForTest(null);
  __setListenForTest(null);
  await testWindow.happyDOM.abort();
  testWindow.close();
}

main().then(() => process.exit(0)).catch((error) => {
  console.error(error);
  process.exit(1);
});
