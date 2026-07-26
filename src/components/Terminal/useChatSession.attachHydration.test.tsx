// Run: npx tsx src/components/Terminal/useChatSession.attachHydration.test.tsx
import type { NormalizedChatMessage } from "@codeterm/chat-core";
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";

function assert(condition: unknown, label: string): asserts condition {
  if (!condition) throw new Error(label);
}

function setupHookDom(): void {
  const hookWindow = new Window({ url: "http://localhost/" });
  Object.defineProperty(globalThis, "IS_REACT_ACT_ENVIRONMENT", {
    configurable: true,
    value: true,
  });
  Object.defineProperty(globalThis, "window", { configurable: true, value: hookWindow });
  Object.defineProperty(globalThis, "document", { configurable: true, value: hookWindow.document });
  Object.defineProperty(globalThis, "navigator", { configurable: true, value: hookWindow.navigator });
  Object.defineProperty(globalThis, "HTMLElement", {
    configurable: true,
    value: hookWindow.HTMLElement,
  });
  Object.defineProperty(globalThis, "Event", { configurable: true, value: hookWindow.Event });
  Object.defineProperty(globalThis, "CustomEvent", {
    configurable: true,
    value: hookWindow.CustomEvent,
  });
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((next) => {
    resolve = next;
  });
  return { promise, resolve };
}

function message(id: string, content = id): NormalizedChatMessage {
  return { id, type: "assistant", content, timestamp: id };
}

interface StartResult {
  text: string;
  start_offset: number;
  offset: number;
  pending: boolean;
  messages?: NormalizedChatMessage[];
  source_session_id: string | null;
  session_binding_nonce: string | null;
  subscription_id?: string | null;
}

type StartFactory = (subscriptionId: string) => Promise<StartResult> | StartResult;
type ChatHandler = (event: {
  payload: {
    pane_id: string;
    messages: NormalizedChatMessage[];
    source_session_id: string | null;
    session_binding_nonce: string | null;
  };
}) => void;

class FakeChatServer {
  readonly starts: string[] = [];
  readonly stops: string[] = [];
  readonly subscribers = new Set<string>();
  readonly retiredHandlers: ChatHandler[] = [];
  private readonly startFactories: StartFactory[] = [];
  handler: ChatHandler | null = null;
  listens = 0;
  unlistens = 0;

  queueStart(factory: StartFactory): void {
    this.startFactories.push(factory);
  }

  install(bridge: {
    __setInvokeForTest: (fn: unknown) => void;
    __setListenForTest: (fn: unknown) => void;
  }): void {
    bridge.__setListenForTest(async (_event: string, handler: ChatHandler) => {
      this.listens += 1;
      this.handler = handler;
      return () => {
        this.unlistens += 1;
        this.retiredHandlers.push(handler);
        if (this.handler === handler) this.handler = null;
      };
    });
    bridge.__setInvokeForTest(async (command: string, args: Record<string, unknown>) => {
      if (command === "chat_start") {
        const subscriptionId = String(args.subscriptionId ?? "");
        this.starts.push(subscriptionId);
        const factory = this.startFactories.shift();
        assert(factory, `unexpected chat_start ${this.starts.length}`);
        const result = await factory(subscriptionId);
        if (result.subscription_id) this.subscribers.add(result.subscription_id);
        return result;
      }
      if (command === "chat_stop") {
        const subscriptionId = String(args.subscriptionId ?? "");
        this.stops.push(subscriptionId);
        this.subscribers.delete(subscriptionId);
      }
      return {};
    });
  }

  emit(
    messages: NormalizedChatMessage[],
    sourceSessionId: string | null,
    sessionBindingNonce: string | null,
    paneId = "pane",
  ): void {
    this.handler?.({
      payload: {
        pane_id: paneId,
        messages,
        source_session_id: sourceSessionId,
        session_binding_nonce: sessionBindingNonce,
      },
    });
  }
}

async function flush(): Promise<void> {
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
    await Promise.resolve();
  });
}

interface Mounted {
  root: Root;
  host: HTMLElement;
  render: (sessionId: string | null, nonce: string | null, active: boolean) => Promise<void>;
  attachmentState: () => string;
  messageIds: () => string[];
}

async function mount(server: FakeChatServer): Promise<Mounted> {
  const bridge = await import("../../lib/bridge");
  const { useChatSession } = await import("./useChatSession");
  server.install(bridge as never);

  function Harness({
    sessionId,
    nonce,
    active,
  }: {
    sessionId: string | null;
    nonce: string | null;
    active: boolean;
  }) {
    const chat = useChatSession({
      paneId: "pane",
      agentSessionId: sessionId,
      sessionBindingNonce: nonce,
      active,
    });
    return createElement(
      "section",
      null,
      createElement("output", { "data-testid": "attachment-state" }, chat.attachmentState),
      ...chat.allMessages.map((row) =>
        createElement(
          "div",
          { key: row.id, "data-testid": "chat-message", "data-message-id": row.id },
          row.content,
        ),
      ),
    );
  }

  const host = document.createElement("main");
  document.body.appendChild(host);
  const root = createRoot(host);
  const render = async (sessionId: string | null, nonce: string | null, active: boolean) => {
    await act(async () => {
      root.render(createElement(Harness, { sessionId, nonce, active }));
    });
    await flush();
  };
  await render(null, null, false);
  return {
    root,
    host,
    render,
    attachmentState: () =>
      host.querySelector('[data-testid="attachment-state"]')?.textContent ?? "",
    messageIds: () =>
      [...host.querySelectorAll('[data-testid="chat-message"]')].map(
        (node) => node.getAttribute("data-message-id") ?? "",
      ),
  };
}

async function main(): Promise<void> {
  setupHookDom();
  const server = new FakeChatServer();
  const app = await mount(server);
  const { fireResync } = await import("../../lib/resync");

  assert(server.starts.length === 0, "hidden Chat must not request a snapshot");
  assert(server.listens === 0, "hidden Chat must not own a listener");

  server.queueStart(() => ({
    text: "",
    start_offset: 0,
    offset: 0,
    pending: true,
    source_session_id: null,
    session_binding_nonce: null,
    subscription_id: null,
  }));
  await app.render(null, null, true);
  assert(
    app.attachmentState() === "awaiting_session",
    `null pane authority must await a session (state=${app.attachmentState()})`,
  );
  assert(app.messageIds().length === 0, "awaiting-session rendered false-success rows");
  await act(async () => {
    server.emit([message("global-candidate")], "newest-global-session", "global-nonce");
  });
  assert(
    app.messageIds().length === 0 && app.attachmentState() === "awaiting_session",
    "unbound pane adopted a global transcript candidate",
  );

  await app.render(null, null, false);
  assert(server.handler === null, "hidden Chat retained its event listener");
  assert(server.subscribers.size === 0, "hidden Chat retained a subscription");

  await app.render("sid-live", "nonce-live", false);
  assert(server.starts.length === 1, "hidden authority commit requested a snapshot");
  server.queueStart((subscriptionId) => ({
    text: "",
    start_offset: 0,
    offset: 1,
    pending: false,
    messages: [message("history")],
    source_session_id: "sid-live",
    session_binding_nonce: "nonce-live",
    subscription_id: subscriptionId,
  }));
  await app.render("sid-live", "nonce-live", true);
  assert(app.attachmentState() === "attached", "bounded snapshot did not attach Chat");
  assert(app.messageIds().join(",") === "history", "pre-existing transcript did not render once");
  assert(server.subscribers.size === 1, "visible Chat did not own exactly one subscription");

  await act(async () => {
    server.emit([message("wrong-pane")], "sid-live", "nonce-live", "foreign-pane");
    server.emit([message("wrong-session")], "sid-foreign", "nonce-live");
    server.emit([message("wrong-nonce")], "sid-live", "nonce-stale");
    server.emit([message("current-delta")], "sid-live", "nonce-live");
  });
  assert(
    app.messageIds().join(",") === "history,current-delta",
    `authority filter adopted a foreign or stale delta (${app.messageIds()})`,
  );

  const startsBeforeResync = server.starts.length;
  server.queueStart((subscriptionId) => ({
    text: "",
    start_offset: 0,
    offset: 2,
    pending: false,
    messages: [message("history"), message("current-delta")],
    source_session_id: "sid-live",
    session_binding_nonce: "nonce-live",
    subscription_id: subscriptionId,
  }));
  await act(async () => fireResync());
  await flush();
  assert(server.starts.length === startsBeforeResync + 1, "resync did not request one snapshot");
  assert(app.messageIds().join(",") === "history,current-delta", "resync duplicated transcript rows");
  assert(server.subscribers.size === 1, "resync left overlapping subscriptions");
  assert(server.stops.includes(server.starts[startsBeforeResync - 1]), "resync did not stop prior subscription");

  const late = deferred<StartResult>();
  await app.render("sid-live", "nonce-live", false);
  server.queueStart(async (subscriptionId) => {
    const result = await late.promise;
    return { ...result, subscription_id: subscriptionId };
  });
  await app.render("sid-live", "nonce-live", true);
  const lateHandler = server.handler;
  const lateSubscription = server.starts.at(-1) ?? "";
  await app.render("sid-live", "nonce-live", false);
  late.resolve({
    text: "",
    start_offset: 0,
    offset: 3,
    pending: false,
    messages: [message("late-snapshot")],
    source_session_id: "sid-live",
    session_binding_nonce: "nonce-live",
  });
  await flush();
  assert(!app.messageIds().includes("late-snapshot"), "late cancelled snapshot was adopted");
  assert(server.stops.includes(lateSubscription), "late cancelled subscription was not stopped");

  server.queueStart((subscriptionId) => ({
    text: "",
    start_offset: 0,
    offset: 2,
    pending: false,
    messages: [message("history"), message("current-delta")],
    source_session_id: "sid-live",
    session_binding_nonce: "nonce-live",
    subscription_id: subscriptionId,
  }));
  await app.render("sid-live", "nonce-live", true);
  await act(async () => {
    lateHandler?.({
      payload: {
        pane_id: "pane",
        messages: [message("late-event")],
        source_session_id: "sid-live",
        session_binding_nonce: "nonce-live",
      },
    });
  });
  assert(!app.messageIds().includes("late-event"), "late prior-lifecycle event was adopted");
  assert(app.messageIds().join(",") === "history,current-delta", "reactivation did not converge");

  await act(async () => app.root.unmount());
  await flush();
  assert(server.handler === null, "unmount retained the listener");
  assert(server.subscribers.size === 0, "unmount retained a subscription");
  console.log(
    `PASS attach hydration: snapshots=${server.starts.length} stops=${server.stops.length} listeners=${server.listens}/${server.unlistens}`,
  );
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
