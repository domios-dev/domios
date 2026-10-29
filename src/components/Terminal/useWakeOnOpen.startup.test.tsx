import { act, Profiler, StrictMode, useEffect, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import { __setInvokeForTest, __setListenForTest } from "../../lib/bridge";
import { createTransportClient } from "../../lib/transportClient";
import { restoreStageView } from "../../lib/restoreStageView";
import type { Tab, ThemeConfig } from "../../lib/types";
import { MuxStateStoreContext, MultiplexerActionsContext, type MultiplexerActionsContextType } from "../../store/multiplexer/context";
import { createMuxStateStore } from "../../store/multiplexer/selectorStore";
import type { ExtendedState } from "../../store/multiplexer/tree";
import { TabContent } from "./TabContent";
import { commitTranscriptBatch, clearTranscriptWindows, transcriptWindowKey } from "./transcriptStore";
import { RestoreStatePanel } from "./RestoreStatePanel";
import { useWakeOnOpen } from "./useWakeOnOpen";
import type { Terminal } from "@xterm/xterm";
import { apiHost } from "../../lib/webApi";
import { bindOutputStreamTransport, resetOutputStream } from "../../lib/outputStream";
import { usePtyConnection } from "../../hooks/terminal/usePtyConnection";
import { useChatSession } from "./useChatSession";
import { ChatSetupState } from "./ChatSetupState";
import { chatBodyView } from "./chatBodyVisibility";
import { Loader } from "../shared/Loader";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const content = vi.hoisted(() => ({ ready: false, cached: false }));
vi.mock("../../hooks/useAgentProviders", () => ({ useAgentProviders: () => ({ providers: [], loading: false, error: null }) }));
vi.mock("./TerminalTab", async () => {
  const { createElement } = await import("react");
  return { TerminalTab: () => createElement("section", { "data-chat-ready": content.ready },
    content.cached ? createElement("div", { "data-chat-window-row": "cached" }, "opaque cached content")
      : content.ready ? createElement("textarea", { "aria-label": "Message composer" })
      : createElement("div", { "data-chat-window-row": "prior" }, "opaque prior content")) };
});

function paneFrame(host: HTMLElement): "error" | "content" | "restoring" | "empty" {
  const composer = host.querySelector('textarea[aria-label="Message composer"]');
  const loading = host.querySelector('[role="status"][aria-busy="true"]');
  if (host.querySelector('[role="alert"]')) return "error";
  if (composer && !composer.closest('[inert], [aria-hidden="true"]')) return "content";
  if (loading && !loading.closest('[aria-hidden="true"]')) return "restoring";
  return "empty";
}

it("recognizes the initial sleeping loader even before wake adds progress steps", async () => {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  try {
    const view = restoreStageView({ frozen: true, agent_type: "fixture" });
    await act(async () => root.render(<RestoreStatePanel view={view} />));
    expect(view.phase).toBe("sleeping");
    expect(host.querySelector('[role="progressbar"]')).toBeNull();
    expect(paneFrame(host)).toBe("restoring");
    await act(async () => root.render(<div aria-hidden="true"><RestoreStatePanel view={view} /></div>));
    expect(paneFrame(host)).toBe("empty");
    await act(async () => root.render(<div />));
    expect(paneFrame(host)).toBe("empty");
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

it("keeps automatic active-tab restore loading until the absent wake route is installed", async () => {
  let subscribed!: () => void;
  const opening = new Promise<void>(resolve => { subscribed = resolve; });
  let ready!: () => void;
  const readiness = new Promise<void>(resolve => { ready = resolve; });
  let attempts = 0;
  const fetch = vi.fn<typeof globalThis.fetch>().mockImplementation(async input => {
    if (String(input).endsWith("/api/startup/events")) {
      subscribed();
      await readiness;
      return new Response('event: startup-state\ndata: {"state":"ready"}\n\n', {
        headers: { "Content-Type": "text/event-stream" },
      });
    }
    return ++attempts === 1
      ? Response.json({ error: { code: "starting", subsystem: "service", request_executed: false } }, { status: 503 })
      : Response.json({ accepted: true });
  });
  const client = createTransportClient({ origin: "http://127.0.0.1:17786", token: "fixture-token", fetch });
  let wake!: Promise<unknown>;
  __setInvokeForTest(<T,>(command: string): Promise<T> => {
    if (command !== "wake_tab") return Promise.resolve({} as T);
    const result = client.json<T>("POST", "/api/tabs/startup-active-fixture/wake");
    wake = result;
    return result;
  });
  const tab = { id: "startup-active-fixture", frozen: true, agent_type: "fixture", agent_session_id: "opaque" } as Tab;
  function ActiveTab() {
    const state = useWakeOnOpen(tab, true);
    return <RestoreStatePanel view={restoreStageView(tab, state)} />;
  }
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<StrictMode><ActiveTab /></StrictMode>));
    await act(async () => { await opening; });
    expect(attempts).toBe(1);
    expect(host.querySelector('[role="progressbar"]')).not.toBeNull();
    expect(host.querySelector('[role="alert"]')).toBeNull();
    expect(host.querySelector("details")).toBeNull();
    await act(async () => { ready(); await wake; });
    expect(attempts).toBe(2);
    expect(host.querySelector('[role="progressbar"]')).not.toBeNull();
    expect(host.querySelector('[role="alert"]')).toBeNull();
  } finally {
    ready();
    await act(async () => root.unmount());
    host.remove();
    __setInvokeForTest(null);
  }
});

it("hands the held active tab directly from restore to ready content with no empty or stale commit", async () => {
  content.ready = false;
  __setListenForTest(async () => () => {});
  let subscribed!: () => void;
  const opening = new Promise<void>(resolve => { subscribed = resolve; });
  let ready!: () => void;
  const readiness = new Promise<void>(resolve => { ready = resolve; });
  let attempts = 0;
  const fetch = vi.fn<typeof globalThis.fetch>().mockImplementation(async input => {
    if (String(input).endsWith("/api/startup/events")) {
      subscribed();
      await readiness;
      return new Response('event: startup-state\ndata: {"state":"ready"}\n\n', { headers: { "Content-Type": "text/event-stream" } });
    }
    return ++attempts === 1
      ? Response.json({ error: { code: "starting", subsystem: "service", request_executed: false } }, { status: 503 })
      : Response.json({ accepted: true });
  });
  const client = createTransportClient({ origin: "http://127.0.0.1:17786", token: "fixture-token", fetch });
  let wake!: Promise<unknown>;
  __setInvokeForTest(<T,>(command: string): Promise<T> => {
    if (command !== "wake_tab") return Promise.resolve({} as T);
    const result = client.json<T>("POST", "/api/tabs/startup-painted-fixture/wake");
    wake = result;
    return result;
  });
  let tab = { id: "startup-painted-fixture", title: "Fixture", tab_type: "Terminal", pty_id: null,
    frozen: true, slept_at_ms: null, backing: { kind: "pty" }, agent_type: "fixture",
    agent_session_id: "opaque", boot_stage: "pty_starting" } as Tab;
  let state: ExtendedState = { workspaces: [], tabs: { [tab.id]: tab }, teams: [], active_workspace_id: null,
    focused_tab_id: tab.id, notifications: [], tabStatuses: {}, toasts: [], activeTabIds: {}, generation: 1, pendingCloses: [] };
  const store = createMuxStateStore(() => state);
  const actions = new Proxy({}, { get: () => () => {} }) as MultiplexerActionsContextType;
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const frames: string[] = [];
  const sample = () => {
    frames.push(paneFrame(host));
    const stale = host.querySelector('[data-chat-window-row="prior"]');
    if (stale && !stale.closest('[inert], [aria-hidden="true"]')) frames.push("stale");
  };
  const render = () => act(async () => root.render(
    <StrictMode><MuxStateStoreContext.Provider value={store}>
      <MultiplexerActionsContext.Provider value={actions}>
        <Profiler id="active-tab" onRender={sample}>
          <TabContent tab={tab} isFocused isVisible onFocusTab={() => {}} onPtySpawned={() => {}}
            theme={{ name: "fixture", background: "#000", foreground: "#fff" } as ThemeConfig} />
        </Profiler>
      </MultiplexerActionsContext.Provider>
    </MuxStateStoreContext.Provider></StrictMode>,
  ));
  try {
    await render();
    await act(async () => { await opening; });
    const box = host.querySelector('[data-tab-id="startup-painted-fixture"]');
    await act(async () => { ready(); await wake; });
    tab = { ...tab, frozen: false, pty_id: "own-pty" };
    state = { ...state, tabs: { [tab.id]: tab }, generation: 2 };
    await act(async () => store.notify());
    await render();
    tab = { ...tab, boot_stage: "agent_ready" };
    state = { ...state, tabs: { [tab.id]: tab }, generation: 3 };
    await act(async () => store.notify());
    await render();
    expect(frames, "commits before the body-ready marker").not.toHaveLength(0);
    expect(frames.every(frame => frame === "restoring"), JSON.stringify(frames)).toBe(true);
    content.ready = true;
    await render();
    expect(host.querySelector('[data-tab-id="startup-painted-fixture"]')).toBe(box);
    const firstContent = frames.indexOf("content");
    expect(firstContent).toBeGreaterThan(-1);
    expect(frames.slice(firstContent).every(frame => frame === "content")).toBe(true);
    expect(frames.every(frame => frame === "restoring" || frame === "content"), JSON.stringify(frames)).toBe(true);
  } finally {
    ready();
    await act(async () => root.unmount());
    host.remove();
    __setInvokeForTest(null);
    content.ready = false;
    __setListenForTest(null);
  }
});

it("keeps cached rows mounted with a restoring pill instead of a full-pane loader", async () => {
  content.cached = true;
  content.ready = false;
  __setListenForTest(async () => () => {});
  let subscribed!: () => void;
  const opening = new Promise<void>(resolve => { subscribed = resolve; });
  let ready!: () => void;
  const readiness = new Promise<void>(resolve => { ready = resolve; });
  let attempts = 0;
  const fetch = vi.fn<typeof globalThis.fetch>().mockImplementation(async input => {
    if (String(input).endsWith("/api/startup/events")) {
      subscribed(); await readiness;
      return new Response('event: startup-state\ndata: {"state":"ready"}\n\n', { headers: { "Content-Type": "text/event-stream" } });
    }
    return ++attempts === 1
      ? Response.json({ error: { code: "starting", subsystem: "service", request_executed: false } }, { status: 503 })
      : Response.json({ accepted: true });
  });
  const client = createTransportClient({ origin: "http://127.0.0.1:17786", token: "fixture-token", fetch });
  let wake!: Promise<unknown>;
  __setInvokeForTest(<T,>(command: string): Promise<T> => {
    if (command !== "wake_tab") return Promise.resolve({} as T);
    const result = client.json<T>("POST", "/api/tabs/startup-cached-fixture/wake");
    wake = result;
    return result;
  });
  let tab = { id: "startup-cached-fixture", title: "Fixture", tab_type: "Terminal", pty_id: null,
    frozen: true, slept_at_ms: null, backing: { kind: "pty" }, agent_type: "fixture",
    agent_session_id: "cached-session", session_binding_nonce: "cached-binding", boot_stage: "pty_starting" } as Tab;
  const key = transcriptWindowKey(tab.id, tab.agent_session_id!);
  commitTranscriptBatch(key, { kind: "snapshot", authority: { sourceSessionId: "cached-session", sessionBindingNonce: "cached-binding" },
    range: null, rows: [{ id: "cached", type: "assistant", content: "opaque cached content" }] });
  let state: ExtendedState = { workspaces: [], tabs: { [tab.id]: tab }, teams: [], active_workspace_id: null,
    focused_tab_id: tab.id, notifications: [], tabStatuses: {}, toasts: [], activeTabIds: {}, generation: 1, pendingCloses: [] };
  const store = createMuxStateStore(() => state);
  const actions = new Proxy({}, { get: () => () => {} }) as MultiplexerActionsContextType;
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const render = () => act(async () => root.render(
    <MuxStateStoreContext.Provider value={store}><MultiplexerActionsContext.Provider value={actions}>
      <TabContent tab={tab} isFocused isVisible onFocusTab={() => {}} onPtySpawned={() => {}}
        theme={{ name: "fixture", background: "#000", foreground: "#fff" } as ThemeConfig} />
    </MultiplexerActionsContext.Provider></MuxStateStoreContext.Provider>,
  ));
  try {
    await render();
    await act(async () => { await opening; });
    const row = host.querySelector('[data-chat-window-row="cached"]');
    expect(row).not.toBeNull();
    expect(row?.closest('[inert], [aria-hidden="true"]')).toBeNull();
    expect(host.querySelector('[role="progressbar"]')).toBeNull();
    expect(host.querySelector('[role="status"][aria-label="Restoring…"]')).not.toBeNull();
    await act(async () => { ready(); await wake; });
    tab = { ...tab, frozen: false, boot_stage: "agent_ready", pty_id: "cached-pty" };
    state = { ...state, tabs: { [tab.id]: tab }, generation: 2 };
    content.ready = true;
    await act(async () => store.notify());
    await render();
    expect(host.querySelector('[data-chat-window-row="cached"]')).toBe(row);
    const pill = host.querySelector('[role="status"][aria-label="Restoring…"]');
    expect(pill === null || pill.closest('[aria-hidden="true"]') !== null).toBe(true);
    expect(host.querySelector('[role="progressbar"]')).toBeNull();
  } finally {
    ready();
    await act(async () => root.unmount());
    host.remove();
    __setInvokeForTest(null);
    __setListenForTest(null);
    clearTranscriptWindows();
    content.cached = false;
    content.ready = false;
  }
});

it.each(["terminal", "chat"] as const)("keeps a real %s attachment loading through startup and connects on readiness", async pane => {
  let subscribed!: () => void;
  const opening = new Promise<void>(resolve => { subscribed = resolve; });
  let ready!: () => void;
  const readiness = new Promise<void>(resolve => { ready = resolve; });
  let connected!: () => void;
  const connection = new Promise<void>(resolve => { connected = resolve; });
  let routesReady = false;
  const tabId = `startup-${pane}-attachment`;
  const fetch = vi.fn<typeof globalThis.fetch>().mockImplementation(async (input, init) => {
    const path = new URL(String(input)).pathname;
    if (path === "/api/startup/events") {
      subscribed(); await readiness;
      return new Response('event: startup-state\ndata: {"state":"ready"}\n\n', { headers: { "Content-Type": "text/event-stream" } });
    }
    if (!routesReady) return Response.json({ error: { code: "starting", subsystem: "service", request_executed: false } }, { status: 503 });
    if (path.endsWith("/chat-raw/start")) {
      const body = JSON.parse(String(init?.body)) as { subscriptionId: string };
      return Response.json({ subscription_id: body.subscriptionId, source_session_id: "own-session", session_binding_nonce: "own-binding",
        messages: [{ id: "own-row", type: "assistant", content: "opaque connected chat" }], text: "", pending: false, start_offset: 0, offset: 20 });
    }
    return new Response(new ReadableStream<Uint8Array>({ start(controller) {
      if (path === "/api/output/stream") controller.enqueue(new TextEncoder().encode(
        `data: ${JSON.stringify({ tab_id: tabId, pty_id: "own-pty", boot: "startup-fixture", offset: 0, replay_complete: true })}\n\n`,
      ));
    } }), { headers: { "Content-Type": "text/event-stream" } });
  });
  const client = createTransportClient({ origin: apiHost(), token: "fixture-token", fetch });
  bindOutputStreamTransport(client);
  __setInvokeForTest(<T,>(command: string, args?: Record<string, unknown>): Promise<T> => command === "chat_start"
    ? client.json<T>("POST", `/api/tabs/${tabId}/chat-raw/start`, args)
    : Promise.resolve([] as T));
  const stops: Array<() => void> = [];
  __setListenForTest(async <T,>(event: string, handler: (event: { payload: T }) => void) => {
    const stop = client.subscribe(`/api/events/stream?event=${encodeURIComponent(event)}`, (_name, payload) => handler({ payload: payload as T }), { requireEventStream: true, onEnd: () => {} });
    stops.push(stop);
    return stop;
  });
  function TerminalPane() {
    const [attached, setAttached] = useState(false);
    const termRef = useRef({ cols: 80, rows: 24,
      buffer: { active: { type: "normal", viewportY: 0, baseY: 0 } },
      scrollToBottom() {}, scrollToLine() {}, writeln() {},
      write(_data: string | Uint8Array, done?: () => void) { done?.(); },
    } as unknown as Terminal);
    const connectedPtyRef = useRef<string | null>(null);
    usePtyConnection({ termRef, connectedPtyRef, tabId, ptyId: "own-pty", termReady: true,
      onShellReady() {}, onOutputChunk() {}, onFrameReady: () => setAttached(true) });
    useEffect(() => { if (attached) connected(); }, [attached]);
    return attached ? <div role="textbox" aria-label="Connected terminal" /> : <Loader label="Starting terminal…" />;
  }
  function ChatPane() {
    const session = useChatSession({ tabId, agentSessionId: "own-session", sessionBindingNonce: "own-binding", active: true, retainAttachment: true });
    const view = chatBodyView({ snapshotReady: session.snapshotReady, hasTranscript: session.allMessages.length > 0,
      chatLoading: session.loading, attachmentError: session.attachmentState === "failed" ? session.error : null,
      unavailable: session.transcriptUnavailable, bootView: { kind: "none" } });
    useEffect(() => { if (session.snapshotReady && !session.loading) connected(); }, [session.snapshotReady, session.loading]);
    return <><ChatSetupState tab={null} providerName="Fixture" view={view} onRetry={() => {}} />
      <div role="log">{session.allMessages.map(message => <p key={message.id}>{message.content}</p>)}</div></>;
  }
  const state = { tabs: {} } as ExtendedState;
  const store = createMuxStateStore(() => state);
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<MuxStateStoreContext.Provider value={store}>{pane === "terminal" ? <TerminalPane /> : <ChatPane />}</MuxStateStoreContext.Provider>));
    await act(async () => { await opening; });
    expect(host.querySelector('[role="alert"]')).toBeNull();
    expect(host.querySelector('[role="status"]')).not.toBeNull();
    await act(async () => { routesReady = true; ready(); });
    await connection;
    expect(host.querySelector('[role="alert"]')).toBeNull();
    if (pane === "terminal") expect(host.querySelector('[role="textbox"][aria-label="Connected terminal"]')).not.toBeNull();
    else expect(host.querySelector('[role="log"]')?.textContent).toContain("opaque connected chat");
  } finally {
    ready();
    await act(async () => root.unmount());
    for (const stop of stops) stop();
    resetOutputStream();
    __setInvokeForTest(null);
    __setListenForTest(null);
    clearTranscriptWindows();
    host.remove();
  }
});
