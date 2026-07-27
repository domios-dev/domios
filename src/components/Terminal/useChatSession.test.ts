import {
  TEST_CHAT_REPLY_EVENT,
  installCodeTermTestChatHandle,
  isCodeTermFrontendTestMode,
  listenForTestChatReplies,
  resetTestChatClockForCodeTerm,
  testChatReplyPayload,
  type ChatSessionDiagnosticEvent,
  type ChatSessionDiagnosticSink,
  useChatSession,
} from "./useChatSession";
import type { NormalizedChatMessage } from "@codeterm/chat-core";
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";

function assert(condition: unknown, label: string): asserts condition {
  if (!condition) throw new Error(label);
}

const testWindow = Object.assign(new EventTarget(), {
  location: {
    protocol: "app:",
    hostname: "",
    port: "",
    search: "?e2e=1",
  },
});

Object.defineProperty(globalThis, "window", {
  configurable: true,
  value: testWindow,
});

class TestCustomEvent<T> extends Event {
  detail: T;

  constructor(type: string, init: CustomEventInit<T>) {
    super(type);
    this.detail = init.detail as T;
  }
}

Object.defineProperty(globalThis, "CustomEvent", {
  configurable: true,
  value: TestCustomEvent,
});

assert(!isCodeTermFrontendTestMode({ DEV: false, VITE_CODETERM_TEST: "1" }), "release ignores leaked chat test flag");
assert(!isCodeTermFrontendTestMode({ DEV: false }), "release ignores runtime e2e query");
assert(isCodeTermFrontendTestMode({ DEV: true, VITE_CODETERM_TEST: "1" }), "dev chat test flag enables test mode");
assert(isCodeTermFrontendTestMode({ DEV: true }), "dev runtime e2e query enables test mode");
testWindow.location = { protocol: "http:", hostname: "127.0.0.1", port: "1420", search: "?e2e=1" };
assert(isCodeTermFrontendTestMode({ DEV: false }), "local dev runtime e2e query enables test mode");
testWindow.location = { protocol: "http:", hostname: "127.0.0.1", port: "7685", search: "?e2e=1" };
assert(!isCodeTermFrontendTestMode({ DEV: false }), "prod port ignores runtime e2e query");
testWindow.location = { protocol: "http:", hostname: "127.0.0.1", port: "1420", search: "?e2e=1" };

installCodeTermTestChatHandle(false);
assert(!getTestChatHandle(), "disabled test mode does not install chat handle");

resetTestChatClockForCodeTerm();
const first = testChatReplyPayload({ tabId: "tab-a", content: "deterministic reply" });
const second = testChatReplyPayload({ tabId: "tab-a", content: "next reply" });

assert(first.tab_id === "tab-a", "payload targets Tab");
assert(first.messages[0]?.type === "assistant", "default reply is assistant");
assert(first.messages[0]?.content === "deterministic reply", "reply content preserved");
assert(first.messages[0]?.timestamp === "1700000000000", "first timestamp is deterministic");
assert(second.messages[0]?.timestamp === "1700000000001", "clock increments deterministically");

let seen = "";
window.addEventListener(TEST_CHAT_REPLY_EVENT, (event) => {
  seen = (event as CustomEvent<{ content: string }>).detail.content;
});
installCodeTermTestChatHandle(true);
getTestChatHandle()?.reply("tab-a", "from handle");

assert(seen === "from handle", "test chat handle dispatches reply event");

let structured: NormalizedChatMessage[] | null = null;
let order = 0;
getTestChatHandle()?.pushMessages("tab-late", [{
  id: "queued-after-reload",
  type: "assistant",
  content: "queued missing ref pl_missingplan",
  timestamp: "1700000000100",
  provider: "test",
} as any]);
const unlistenLate = listenForTestChatReplies(
  "tab-late",
  (updater) => {
    structured = typeof updater === "function" ? updater(structured) : updater;
  },
  () => order++,
);

const drainedStructured = structured as NormalizedChatMessage[] | null;
assert(
  drainedStructured?.some((message) => message.id === "queued-after-reload" && message.clientOrder === 0),
  "queued test chat message drains when Tab listener mounts after reload",
);
unlistenLate?.();

function getTestChatHandle() {
  return window.__codetermTestChat;
}

type TimerJob = { due: number; callback: () => void };

class FakeTimers {
  private now = 0;
  private nextId = 1;
  private jobs = new Map<number, TimerJob>();

  install(): () => void {
    const setTimeoutOriginal = globalThis.setTimeout;
    const clearTimeoutOriginal = globalThis.clearTimeout;
    Object.defineProperty(globalThis, "setTimeout", {
      configurable: true,
      value: (callback: () => void, delay = 0) => {
        const id = this.nextId++;
        this.jobs.set(id, { due: this.now + Number(delay), callback });
        return id;
      },
    });
    Object.defineProperty(globalThis, "clearTimeout", {
      configurable: true,
      value: (id: number) => this.jobs.delete(id),
    });
    return () => {
      Object.defineProperty(globalThis, "setTimeout", { configurable: true, value: setTimeoutOriginal });
      Object.defineProperty(globalThis, "clearTimeout", { configurable: true, value: clearTimeoutOriginal });
    };
  }

  async advance(ms: number): Promise<void> {
    const deadline = this.now + ms;
    while (true) {
      const next = [...this.jobs.entries()]
        .filter(([, job]) => job.due <= deadline)
        .sort(([, a], [, b]) => a.due - b.due)[0];
      if (!next) break;
      const [id, job] = next;
      this.jobs.delete(id);
      this.now = job.due;
      await act(async () => {
        job.callback();
        await Promise.resolve();
        await Promise.resolve();
      });
    }
    this.now = deadline;
    await act(async () => {
      await Promise.resolve();
      await Promise.resolve();
    });
  }
}

type SessionView = {
  loading: boolean;
  recovering: boolean;
  messages: NormalizedChatMessage[];
};

type ChatStartMock = {
  text: string;
  offset: number;
  pending?: boolean;
  messages?: NormalizedChatMessage[];
  source_session_id: string | null;
  session_binding_nonce: string | null;
  subscription_id: string | null;
};

function chatStartResult<T extends { text: string; offset: number }>(sourceSessionId: string | null, result: T, sessionBindingNonce: string | null = null): T & ChatStartMock {
  return {
    ...result,
    source_session_id: sourceSessionId,
    session_binding_nonce: sessionBindingNonce,
    subscription_id: "test-subscription",
  };
}

function chatEvent(sourceSessionId: string | null, messages: NormalizedChatMessage[], sessionBindingNonce: string | null = null) {
  return { tab_id: "tab", source_session_id: sourceSessionId, session_binding_nonce: sessionBindingNonce, messages };
}

function setupHookDom(): void {
  const hookWindow = new Window({ url: "http://localhost/" });
  Object.defineProperty(globalThis, "IS_REACT_ACT_ENVIRONMENT", { configurable: true, value: true });
  Object.defineProperty(globalThis, "window", { configurable: true, value: hookWindow });
  Object.defineProperty(globalThis, "document", { configurable: true, value: hookWindow.document });
  Object.defineProperty(globalThis, "navigator", { configurable: true, value: hookWindow.navigator });
  Object.defineProperty(globalThis, "HTMLElement", { configurable: true, value: hookWindow.HTMLElement });
  Object.defineProperty(globalThis, "Event", { configurable: true, value: hookWindow.Event });
  Object.defineProperty(globalThis, "CustomEvent", { configurable: true, value: hookWindow.CustomEvent });
}

async function flush(): Promise<void> {
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
  });
}

async function mountSession(
  sessionId: string | null,
  active = true,
  sessionBindingNonce: string | null = null,
  diagnosticSink?: ChatSessionDiagnosticSink,
): Promise<{ root: Root; render: (nextSessionId: string | null, nextActive?: boolean, nextSessionBindingNonce?: string | null) => Promise<void>; latest: () => SessionView }> {
  let view: SessionView = { loading: true, recovering: false, messages: [] };
  function Harness({ currentSessionId, currentActive, currentSessionBindingNonce }: { currentSessionId: string | null; currentActive: boolean; currentSessionBindingNonce: string | null }) {
    const session = useChatSession({ tabId: "tab", agentSessionId: currentSessionId, sessionBindingNonce: currentSessionBindingNonce, active: currentActive, diagnosticSink });
    view = { loading: session.loading, recovering: session.recovering, messages: session.allMessages };
    return null;
  }
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  const render = async (nextSessionId: string | null, nextActive = true, nextSessionBindingNonce: string | null = null) => {
    await act(async () => {
      root.render(createElement(Harness, { currentSessionId: nextSessionId, currentActive: nextActive, currentSessionBindingNonce: nextSessionBindingNonce }));
    });
    await flush();
  };
  await render(sessionId, active, sessionBindingNonce);
  return { root, render, latest: () => view };
}

async function runChatSessionHookTests(): Promise<void> {
  setupHookDom();
  const { __setInvokeForTest, __setListenForTest } = await import("../../lib/bridge");
  const { fireResync } = await import("../../lib/resync");

  {
    let starts = 0;
    __setListenForTest(async () => () => {});
    __setInvokeForTest((async (command: string) => {
      if (command !== "chat_start") return {};
      starts += 1;
      return chatStartResult("sid-1", {
        text: "",
        offset: starts,
        messages: [{
          id: starts === 1 ? "before-resync" : "after-resync",
          type: "assistant",
          content: starts === 1 ? "before" : "fresh from snapshot",
          timestamp: String(starts),
        }],
      });
    }) as never);
    const session = await mountSession("sid-1");
    await act(async () => fireResync());
    await flush();
    assert(starts === 2, "web event loss requests one fresh chat snapshot");
    assert(
      session.latest().messages.some((message) => message.content === "fresh from snapshot"),
      "web event loss refreshes the visible transcript",
    );
    await act(async () => session.root.unmount());
    console.log("PASS web event loss refreshes the active chat snapshot");
  }

  {
    let handler: ((event: { payload: { tab_id: string; messages: NormalizedChatMessage[] } }) => void) | null = null;
    __setListenForTest(async (_event, nextHandler) => {
      handler = nextHandler as typeof handler;
      return () => {};
    });
    __setInvokeForTest((async (command: string) => {
      if (command !== "chat_start") return {};
      return chatStartResult("resumed-rollout", {
        text: "",
        offset: 1,
        messages: [{ id: "snapshot", type: "assistant", content: "resumed snapshot", timestamp: "1" }],
      }, "new-launch-nonce");
    }) as never);
    const session = await mountSession("resumed-from", true, "old-launch-nonce");
    assert(
      session.latest().messages.length === 0,
      "chat_start cannot replace the Tab's current session authority",
    );
    await session.render("resumed-rollout", true, "new-launch-nonce");
    await act(async () => handler?.({ payload: chatEvent("resumed-rollout", [{
      id: "live",
      type: "assistant",
      content: "live resumed reply",
      timestamp: "2",
    }], "rotated-launch-nonce") }));
    assert(
      session.latest().messages.map((message) => message.content).join(",") === "resumed snapshot",
      "only the tab-confirmed session and binding nonce can update the transcript",
    );
    await act(async () => session.root.unmount());
    console.log("PASS Tab authority gates snapshot handoff and nonce rotation");
  }

  {
    const timers = new FakeTimers();
    const restoreTimers = timers.install();
    let starts = 0;
    let listens = 0;
    let stops = 0;
    let unlistens = 0;
    __setListenForTest(async () => {
      listens += 1;
      return () => { unlistens += 1; };
    });
    __setInvokeForTest((async (command: string) => {
      if (command === "chat_start") {
        starts += 1;
        return chatStartResult("sid-1", { text: "", offset: 0, pending: true });
      }
      if (command === "chat_stop") stops += 1;
      return {};
    }) as never);
    const session = await mountSession("sid-1");
    await timers.advance(30_000);
    assert(listens === 1, "T1 owns one persistent listener while pending");
    assert(unlistens === 0 && stops === 0, "T1 pending does not tear down the listener or chat");
    assert(starts === 1, "T1 pending never polls chat_start after the initial attach");
    assert(session.latest().recovering && !session.latest().loading, "T1 pending settles as recovering without loading");
    await act(async () => session.root.unmount());
    restoreTimers();
    console.log("PASS T1 pending waits with one listener and one attach");
  }

  {
    const timers = new FakeTimers();
    const restoreTimers = timers.install();
    let starts = 0;
    let handler: ((event: { payload: { tab_id: string; messages: NormalizedChatMessage[] } }) => void) | null = null;
    __setListenForTest(async (_event, nextHandler) => {
      handler = nextHandler as typeof handler;
      return () => {};
    });
    __setInvokeForTest((async (command: string) => {
      if (command === "chat_start") {
        starts += 1;
        return chatStartResult("sid-1", { text: "", offset: 0, pending: true });
      }
      return {};
    }) as never);
    const session = await mountSession("sid-1");
    await act(async () => handler?.({ payload: chatEvent("sid-1", [{ id: "event", type: "assistant", content: "attached", timestamp: "1" }]) }));
    await timers.advance(30_000);
    assert(starts === 1, "T2 first tab-chat message attaches without polling");
    assert(session.latest().messages[0]?.content === "attached" && !session.latest().recovering, "T2 event attaches and renders messages");
    await act(async () => session.root.unmount());
    restoreTimers();
    console.log("PASS T2 Tab event completes an awaiting-store attach");
  }

  {
    let handler: ((event: { payload: { tab_id: string; messages: NormalizedChatMessage[]; source_session_id: string | null } }) => void) | null = null;
    __setListenForTest(async (_event, nextHandler) => {
      handler = nextHandler as typeof handler;
      return () => {};
    });
    __setInvokeForTest((async (command: string) => {
      if (command === "chat_start") return chatStartResult(null, { text: "", offset: 0, pending: true });
      return {};
    }) as never);
    const session = await mountSession(null);
    await act(async () => handler?.({ payload: chatEvent("materialized-session", [{
      id: "live-after-pending",
      type: "assistant",
      content: "materialized without remount",
      timestamp: "1",
    }]) }));
    assert(session.latest().messages.length === 0, "an unbound Tab rejects a global live delta");
    await act(async () => session.root.unmount());
    console.log("PASS unbound Tab has no global transcript authority");
  }

  {
    const startArgs: Array<Record<string, unknown> | undefined> = [];
    const stopArgs: Array<Record<string, unknown> | undefined> = [];
    __setListenForTest(async () => () => {});
    __setInvokeForTest((async (command: string, args?: Record<string, unknown>) => {
      if (command === "chat_start") {
        startArgs.push(args);
        return {
          ...chatStartResult("sid-1", { text: "", offset: 0, messages: [] }),
          subscription_id: args?.subscriptionId,
        };
      }
      if (command === "chat_stop") stopArgs.push(args);
      return {};
    }) as never);
    const session = await mountSession("sid-1");
    await act(async () => session.root.unmount());
    assert(
      startArgs[0]?.subscriptionId === stopArgs[0]?.subscriptionId,
      "one mounted chat releases only the exact lease it acquired",
    );
    console.log("PASS mounted chat start and stop use one lease");
  }

  {
    const timers = new FakeTimers();
    const restoreTimers = timers.install();
    const startArgs: Array<Record<string, unknown> | undefined> = [];
    let stops = 0;
    let unlistens = 0;
    __setListenForTest(async () => () => { unlistens += 1; });
    __setInvokeForTest((async (command: string, args?: Record<string, unknown>) => {
      if (command === "chat_start") {
        startArgs.push(args);
        return {
          ...chatStartResult(
            String(args?.agentSessionId),
            { text: "", offset: 0, pending: true },
          ),
          subscription_id: args?.subscriptionId,
        };
      }
      if (command === "chat_stop") stops += 1;
      return {};
    }) as never);
    const session = await mountSession("sid-1");
    await session.render("sid-2");
    await session.render("sid-3");
    assert(stops === 2 && unlistens === 0, "T3 rotations release only stale subscriptions");
    await timers.advance(0);
    assert(startArgs.length === 3 && startArgs[2]?.agentSessionId === "sid-3", "T3 resyncs each explicit pending session rotation");
    assert(stops === 2 && unlistens === 0, "T3 rotation keeps listener ownership singular");
    await act(async () => session.root.unmount());
    restoreTimers();
    console.log("PASS T3 pending rotations replace subscriptions without replacing the listener");
  }

  {
    let starts = 0;
    let resolveResync!: () => void;
    const resynced = new Promise<void>((resolve) => {
      resolveResync = resolve;
    });
    let handler: ((event: { payload: { tab_id: string; messages: NormalizedChatMessage[] } }) => void) | null = null;
    __setListenForTest(async (_event, nextHandler) => {
      handler = nextHandler as typeof handler;
      return () => {};
    });
    __setInvokeForTest((async (command: string) => {
      if (command !== "chat_start") return {};
      starts += 1;
      return starts === 1
        ? chatStartResult("sid-1", { text: "", offset: 1, messages: [{ id: "one", type: "assistant", content: "one", timestamp: "1" }] })
        : chatStartResult("sid-2", { text: "", offset: 3, messages: [{ id: "one", type: "assistant", content: "one updated", timestamp: "1" }, { id: "three", type: "assistant", content: "three", timestamp: "3" }] });
    }) as never);
    const diagnosticSink = (event: ChatSessionDiagnosticEvent) => {
      if (event.edge === "state_commit" && event.messageIds?.includes("three")) resolveResync();
    };
    const session = await mountSession("sid-1", true, null, diagnosticSink);
    await act(async () => handler?.({ payload: chatEvent("sid-1", [{ id: "two", type: "assistant", content: "two", timestamp: "2" }]) }));
    await session.render("sid-2");
    await resynced;
    const messages = session.latest().messages;
    assert(starts === 2, "T4 attached rotation performs one coalesced resync");
    assert(messages.map((message) => message.id).join(",") === "one,two,three", "T4 resync preserves transcript ordering");
    assert(messages.map((message) => message.clientOrder).join(",") === "0,1,2", "T4 resync preserves monotonic client order");
    await act(async () => session.root.unmount());
    console.log("PASS T4 attached rotation preserves transcript order");
  }

  {
    let starts = 0;
    let stops = 0;
    __setListenForTest(async () => () => {});
    __setInvokeForTest((async (command: string) => {
      if (command === "chat_start") {
        starts += 1;
        return chatStartResult("sid-1", { text: "", offset: 1, messages: [{ id: "history", type: "assistant", content: "kept", timestamp: "1" }] });
      }
      if (command === "chat_stop") stops += 1;
      return {};
    }) as never);
    const session = await mountSession("sid-1");
    await session.render("sid-1", false);
    await session.render("sid-1", true);
    assert(stops === 1 && starts === 2, "T5 visibility transition performs one stop/start pair");
    assert(session.latest().messages[0]?.content === "kept", "T5 visibility transition retains transcript");
    await act(async () => session.root.unmount());
    console.log("PASS T5 visibility restart retains transcript");
  }

  {
    const timers = new FakeTimers();
    const restoreTimers = timers.install();
    const startArgs: Array<Record<string, unknown> | undefined> = [];
    const initialAttach = {
      resolve: null as ((result: ChatStartMock) => void) | null,
    };
    let stops = 0;
    let unlistens = 0;
    __setListenForTest(async () => () => { unlistens += 1; });
    __setInvokeForTest((async (command: string, args?: Record<string, unknown>) => {
      if (command === "chat_start") {
        startArgs.push(args);
        if (startArgs.length === 1) {
          return new Promise((resolve) => { initialAttach.resolve = resolve; });
        }
        return chatStartResult("sid-3", { text: "", offset: 0, pending: true }, "nonce-2");
      }
      if (command === "chat_stop") stops += 1;
      return {};
    }) as never);
    const session = await mountSession("sid-1", true, "nonce-1");
    await session.render("sid-2", true, "nonce-2");
    await session.render("sid-3", true, "nonce-2");
    await timers.advance(0);
    assert(startArgs.length === 3 && startArgs[2]?.agentSessionId === "sid-3", "T6 deferred initial attach reaches the latest session");
    assert(stops === 1 && unlistens === 0, "T6 idle rotation releases only the superseded subscription");
    initialAttach.resolve?.(chatStartResult("sid-1", { text: "", offset: 0, pending: true }, "nonce-1"));
    await flush();
    await timers.advance(30_000);
    assert(startArgs.length === 3, "T6 resolving a stale initial attach does not poll or reattach again");
    assert(stops === 1 && unlistens === 0, "T6 stale completion cannot stop the active subscription");
    await act(async () => session.root.unmount());
    restoreTimers();
    console.log("PASS T6 deferred idle rotations preserve the active lease without polling");
  }

  {
    const timers = new FakeTimers();
    const restoreTimers = timers.install();
    const startArgs: Array<Record<string, unknown> | undefined> = [];
    const retryAttach = {
      resolve: null as ((result: ChatStartMock) => void) | null,
    };
    let stops = 0;
    let unlistens = 0;
    __setListenForTest(async () => () => { unlistens += 1; });
    __setInvokeForTest((async (command: string, args?: Record<string, unknown>) => {
      if (command === "chat_start") {
        startArgs.push(args);
        if (startArgs.length === 2) {
          return new Promise((resolve) => { retryAttach.resolve = resolve; });
        }
        return chatStartResult("sid-1", { text: "", offset: 0, messages: [] });
      }
      if (command === "chat_stop") stops += 1;
      return {};
    }) as never);
    const session = await mountSession("sid-1");
    await act(async () => window.dispatchEvent(new CustomEvent("codeterm:chat-reattached", { detail: { tabId: "tab" } })));
    await flush();
    await session.render("sid-2");
    await session.render("sid-3");
    await timers.advance(0);
    assert(startArgs.length === 4 && startArgs[3]?.agentSessionId === "sid-3", "T7 attaching rotation reaches the latest session");
    assert(
      Number(stops) === 3 && unlistens === 0,
      `T7 attaching rotation releases each superseded subscription (${stops}/${unlistens})`,
    );
    retryAttach.resolve?.(chatStartResult("sid-1", { text: "", offset: 0, pending: true }));
    await flush();
    await timers.advance(30_000);
    assert(startArgs.length === 4, "T7 attaching rotation does not introduce polling");
    assert(
      Number(stops) === 4 && unlistens === 0,
      `T7 stale completion releases its own superseded subscription (${stops}/${unlistens})`,
    );
    await act(async () => session.root.unmount());
    restoreTimers();
    console.log("PASS T7 attaching rotations preserve listener ownership");
  }

  {
    const timers = new FakeTimers();
    const restoreTimers = timers.install();
    const startArgs: Array<Record<string, unknown> | undefined> = [];
    let stops = 0;
    let unlistens = 0;
    __setListenForTest(async () => () => { unlistens += 1; });
    __setInvokeForTest((async (command: string, args?: Record<string, unknown>) => {
      if (command === "chat_start") {
        startArgs.push(args);
        if (startArgs.length === 1) throw new Error("initial attach failed");
        return chatStartResult("sid-3", { text: "", offset: 0, pending: true });
      }
      if (command === "chat_stop") stops += 1;
      return {};
    }) as never);
    const session = await mountSession("sid-1");
    await session.render("sid-2");
    await session.render("sid-3");
    await timers.advance(0);
    assert(startArgs.length === 3 && startArgs[2]?.agentSessionId === "sid-3", "T8 failed rotation reaches the latest session");
    assert(
      stops === 1 && unlistens === 0,
      `T8 failed rotation releases only the superseded subscription (${stops}/${unlistens})`,
    );
    await timers.advance(30_000);
    assert(startArgs.length === 3, "T8 failed rotation keeps no-polling behavior");
    await act(async () => session.root.unmount());
    restoreTimers();
    console.log("PASS T8 failed rotations preserve listener ownership without polling");
  }
  {
    const timers = new FakeTimers();
    const restoreTimers = timers.install();
    let handler: ((event: { payload: { tab_id: string; messages: NormalizedChatMessage[] } }) => void) | null = null;
    __setListenForTest(async (_event, nextHandler) => {
      handler = nextHandler as typeof handler;
      return () => {};
    });
    const startupText = "You are Theo. You are a frontend worker fixing a transcript projection/dedup defect.";
    let starts = 0;
    __setInvokeForTest((async (command: string) => {
      if (command === "chat_start") {
        starts += 1;
        return chatStartResult(starts === 1 ? null : "c73becde", { text: "", offset: 1, messages: [{
          id: starts === 1 ? "snapshot-provisional-user-1" : "snapshot-bound-user-1",
          type: "user", content: startupText, seq: 1,
        }] });
      }
      return {};
    }) as never);
    const session = await mountSession(null);
    await session.render("c73becde");
    await timers.advance(0);
    await act(async () => {
      handler?.({ payload: chatEvent("c73becde", [{
        id: "live-bound-user-1", type: "user", content: startupText, seq: 1,
      }]) });
    });
    assert(
      starts === 2
        && session.latest().messages.length === 1
        && session.latest().messages[0]?.content === startupText,
      "P0 provisional-to-bound reattach keeps one visible attachment lineage",
    );
    await act(async () => session.root.unmount());
    restoreTimers();
    console.log("PASS P0 provisional-to-bound reattach keeps attachment lineage");
  }

  {
    let handler: ((event: { payload: { tab_id: string; messages: NormalizedChatMessage[] } }) => void) | null = null;
    __setListenForTest(async (_event, nextHandler) => {
      handler = nextHandler as typeof handler;
      return () => {};
    });
    const startupText = "You are Theo. You are a frontend worker fixing a transcript projection/dedup defect.";
    __setInvokeForTest((async (command: string) => {
      if (command === "chat_start") {
        return chatStartResult("c73becde", { text: "", offset: 1, messages: [{
          id: "snapshot-c73becde-user-1", type: "user", content: startupText, seq: 1,
        }] });
      }
      return {};
    }) as never);
    const session = await mountSession("c73becde");
    await act(async () => {
      handler?.({ payload: chatEvent("c73becde", [{
        id: "live-c73becde-user-1", type: "user", content: startupText, seq: 1,
      }]) });
    });
    assert(
      session.latest().messages.map((message) => message.id).join(",") === "live-c73becde-user-1",
      "P0 launch snapshot/live pair projects one stable seq-1 user bubble",
    );
    await act(async () => {
      handler?.({ payload: chatEvent("c73becde", [
        { id: "repeat-user-2", type: "user", content: "continue", seq: 2 },
        { id: "repeat-user-3", type: "user", content: "continue", seq: 3 },
      ]) });
    });
    assert(
      session.latest().messages.filter((message) => message.content === "continue").length === 2,
      "P0 distinct user sequences preserve true repeated text",
    );
    await act(async () => session.root.unmount());
    console.log("PASS P0 snapshot/live stable identity reconciliation");
  }
  __setInvokeForTest(null);
  __setListenForTest(null);
}

void runChatSessionHookTests()
  .then(() => console.log("chat test-mode seam checks passed"))
  .catch((error) => {
    console.error(error);
    process.exitCode = 1;
  });
