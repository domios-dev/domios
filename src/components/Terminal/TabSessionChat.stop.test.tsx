// Run: npx tsx src/components/Terminal/TabSessionChat.stop.test.tsx

import { act } from "react";
import { Window } from "happy-dom";
import { createRoot } from "react-dom/client";
import type { Tab } from "../../lib/types";
import { MultiplexerActionsContext, MuxStateStoreContext, type MultiplexerActionsContextType } from "../../store/multiplexer/context";
import { createMuxStateStore } from "../../store/multiplexer/selectorStore";
import type { ExtendedState } from "../../store/multiplexer/tree";
import { remoteTransportCall } from "../../lib/remoteTabTransport";

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
  let receipt: unknown;
  __setInvokeForTest((async (cmd: string, args?: unknown) => {
    if (cmd === "interrupt_agent") { interrupts.push(args); return receipt; }
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
    notifications: [], tabStatuses: { [tab.id]: "Working" }, toasts: [], activeTabIds: {}, generation: 1, pendingCloses: [],
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
  console.log("PASS local working chat_backend tab renders an accessible Stop that interrupts once");

  await act(async () => root.unmount());
  host.remove();

  for (const provider of ["acp", "opencode", "claude"]) {
    const remoteTabId = `mesh:install-b/${provider}-chat`;
    const remoteTab = {
      ...tab,
      id: remoteTabId,
      agent_type: provider,
      session_binding_nonce: "binding-1",
      agent_runtime_state: { state: "loaded", epoch: 2, active_turn_started_at_ms: 500 },
      backing: { kind: "chat_backend", plugin_id: provider, session_id: "thread-1" },
      remote_peer_id: "install-b",
      remote_peer_name: "Peer B",
      remote_tab_id: `${provider}-chat`,
      remote_channels: { control: "available", chat: "available" },
    } as Tab;
    const remoteState = {
      ...state,
      tabs: { [remoteTabId]: remoteTab },
      focused_tab_id: remoteTabId,
      tabStatuses: { [remoteTabId]: "Working" },
    } as unknown as ExtendedState;
    const remoteHost = document.createElement("div");
    document.body.appendChild(remoteHost);
    const remoteRoot = createRoot(remoteHost);
    interrupts.length = 0;
    const confirmed = provider === "claude";
    receipt = {
      stopped: provider !== "opencode",
      interrupted_turn: {
        session_id: "thread-1", session_binding_nonce: "binding-1", epoch: 2,
        started_at_ms: provider === "acp" ? 400 : 500,
      },
    };
    await act(async () => {
      remoteRoot.render(
        <MuxStateStoreContext.Provider value={createMuxStateStore(() => remoteState)}>
          <MultiplexerActionsContext.Provider value={actions}>
            <TabSessionChat tabId={remoteTabId} chatBackend isVisible />
          </MultiplexerActionsContext.Provider>
        </MuxStateStoreContext.Provider>,
      );
    });
    await act(async () => { await Promise.resolve(); });
    const remoteStops = [...remoteHost.querySelectorAll("button")].filter((button) =>
      button.getAttribute("aria-label") === "Stop the agent" && !button.disabled && button.getAttribute("aria-hidden") !== "true");
    assert(remoteStops.length === 1, `${provider} remote Working snapshot renders one accessible Stop`);
    await act(async () => { remoteStops[0].click(); });
    assert(JSON.stringify(interrupts[0]) === JSON.stringify({ tabId: remoteTabId }), `${provider} Stop preserves exact composite tab id`);
    const route = remoteTransportCall("interrupt_agent", interrupts[0] as Record<string, unknown>);
    assert(route?.peerId === "install-b", `${provider} Stop routes to the selected installation`);
    assert(route.remoteTabId === `${provider}-chat`, `${provider} Stop uses the peer's raw tab id`);
    assert(route.method === "POST" && route.suffix === "/stop", `${provider} Stop uses the peer interruption endpoint`);
    const enabled = remoteHost.querySelector<HTMLButtonElement>('button[aria-label="Stop the agent"]:not(:disabled)');
    assert(Boolean(enabled) === !confirmed, `${provider} Stop settles only from the confirmed current turn`);
    if (confirmed) assert(remoteHost.querySelector('[role="status"][aria-label*="Interrupted"]'), "the composer shows interruption before any mux refresh");
    console.log(`PASS remote ${provider} Working snapshot renders Stop and routes to its exact peer tab`);
    await act(async () => remoteRoot.unmount());
    remoteHost.remove();
  }

  __setInvokeForTest(null);
  __setListenForTest(null);
  await testWindow.happyDOM.abort();
  testWindow.close();
}

main().then(() => process.exit(0)).catch((error) => {
  console.error(error);
  process.exit(1);
});
