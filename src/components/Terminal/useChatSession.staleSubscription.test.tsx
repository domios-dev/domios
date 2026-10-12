// A subscription that outlives its session binding is released and replaced on the first observed edge — never on a deadline — for a ranged and an unranged source through the same code path.
import type { NormalizedChatMessage } from "@codeterm/chat-core";
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";

function assert(condition: unknown, label: string): asserts condition {
  if (!condition) throw new Error(label);
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

type ChatHandler = (event: { payload: unknown }) => void;

function row(id: string, content: string): NormalizedChatMessage {
  return { id, type: "assistant", content };
}

interface Binding {
  sessionId: string | null;
  nonce: string | null;
}

class FakeChatServer {
  handler: ChatHandler | null = null;
  statusHandler: ChatHandler | null = null;
  statuses: import("./chatSendStatus").ChatSendStatus[] = [];
  starts: string[] = [];
  stops: string[] = [];
  live = new Set<string>();
  offset = 100;
  binding: Binding = { sessionId: "sid-1", nonce: "nonce-1" };
  transcript: NormalizedChatMessage[] = [];

  constructor(readonly ranged: boolean) {}

  install(bridge: {
    __setInvokeForTest: (fn: unknown) => void;
    __setListenForTest: (fn: unknown) => void;
  }): void {
    bridge.__setListenForTest(async (event: string, next: ChatHandler) => {
      if (event === "chat-send-status") {
        this.statusHandler = next;
        return () => { if (this.statusHandler === next) this.statusHandler = null; };
      }
      if (event !== "tab-chat-msg-tab") return () => {};
      this.handler = next;
      return () => {
        if (this.handler === next) this.handler = null;
      };
    });
    bridge.__setInvokeForTest(async (command: string, args: Record<string, unknown>) => {
      if (command === "chat_start") {
        const subscriptionId = String(args.subscriptionId ?? "");
        this.starts.push(subscriptionId);
        if (this.binding.sessionId != null) this.live.add(subscriptionId);
        return {
          text: "",
          start_offset: this.ranged ? 0 : undefined,
          offset: this.ranged ? this.offset : 0,
          pending: this.binding.sessionId == null,
          messages: [...this.transcript],
          source_session_id: this.binding.sessionId,
          session_binding_nonce: this.binding.nonce,
          subscription_id: this.binding.sessionId == null ? null : subscriptionId,
        };
      }
      if (command === "chat_send_status") return this.statuses;
      if (command === "chat_stop") {
        const subscriptionId = String(args.subscriptionId ?? "");
        this.stops.push(subscriptionId);
        this.live.delete(subscriptionId);
      }
      return {};
    });
  }

  /** Emit under an explicit binding: the stale subscription keeps answering for
   *  the binding it was created with. */
  emit(rows: NormalizedChatMessage[], binding: Binding): void {
    const start = this.offset;
    const end = start + rows.length * 10;
    this.offset = end;
    this.handler?.({
      payload: {
        tab_id: "tab",
        messages: rows,
        start_offset: this.ranged ? start : undefined,
        offset: this.ranged ? end : undefined,
        source_session_id: binding.sessionId,
        session_binding_nonce: binding.nonce,
      },
    });
  }

  rebind(binding: Binding, transcript: NormalizedChatMessage[]): void {
    this.binding = binding;
    this.transcript = transcript;
    this.offset = 100;
  }
}

async function flush(): Promise<void> {
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
    await Promise.resolve();
  });
}

async function runVariant(ranged: boolean, held: boolean): Promise<void> {
  const label = `${ranged ? "ranged" : "unranged"}, held=${held}`;
  const bridge = await import("../../lib/bridge");
  const { useChatSession } = await import("./useChatSession");
  const { clearTranscriptWindows } = await import("./transcriptStore");
  const { addChatSend, readChatSends, forgetChatSendsInMemory } = await import("./chatSendStore");
  const { deliveryState } = await import("./chatDeliveryModel");
  clearTranscriptWindows();
  window.localStorage.clear();
  forgetChatSendsInMemory();

  const scheduled: string[] = [];
  const realSetTimeout = globalThis.setTimeout;
  const realSetInterval = globalThis.setInterval;
  Object.defineProperty(globalThis, "setTimeout", {
    configurable: true,
    value: (...args: unknown[]) => {
      scheduled.push("setTimeout");
      return (realSetTimeout as never as (...a: unknown[]) => number)(...args);
    },
  });
  Object.defineProperty(globalThis, "setInterval", {
    configurable: true,
    value: (...args: unknown[]) => {
      scheduled.push("setInterval");
      return (realSetInterval as never as (...a: unknown[]) => number)(...args);
    },
  });

  const stale: Binding = { sessionId: "sid-1", nonce: "nonce-1" };
  const server = new FakeChatServer(ranged);
  server.transcript = [row("s-1", "old session history")];
  server.install(bridge as never);
  if (held) for (const id of ["one", "two"]) addChatSend("tab", { id, correlationId: id, content: id, sentAtMs: 1 });

  let view: NormalizedChatMessage[] = [];
  function Harness({ sid, nonce }: { sid: string | null; nonce: string | null }) {
    const session = useChatSession({
      tabId: "tab",
      agentSessionId: sid,
      sessionBindingNonce: nonce,
      active: true,
    });
    view = session.allMessages;
    return null;
  }
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root: Root = createRoot(host);
  const render = async (sid: string | null, nonce: string | null) => {
    await act(async () => {
      root.render(createElement(Harness, { sid, nonce }));
    });
    await flush();
  };
  await render(stale.sessionId, stale.nonce);
  const staleSubscription = server.starts[server.starts.length - 1] ?? "";
  assert(view.some((m) => m.content === "old session history"), `${label}: first binding renders`);

  server.rebind({ sessionId: null, nonce: "nonce-2" }, []);
  await render(null, "nonce-2");
  if (held) {
    server.statuses = ["one", "two"].map((id, sequence) => ({ send_id: id, tab_id: "tab",
      correlationId: id, ledger_id: "ledger", sequence, retained_from: 0,
      state: "failed", error: "launch_ended_before_write" }));
    await act(async () => {
      for (const status of server.statuses) server.statusHandler?.({ payload: status });
    });
    assert(readChatSends("tab").every((send) => deliveryState(send) === "failed"), `${label}: failures apply on their event before fresh readiness`);
  }
  const fresh: Binding = { sessionId: "sid-2", nonce: "nonce-2" };
  server.rebind(fresh, [row("n-1", "new session history")]);
  await render(fresh.sessionId, fresh.nonce);

  assert(
    server.stops.filter((id) => id === staleSubscription).length === 1,
    `${label}: the superseded subscription is released exactly once (${server.stops.join(",")})`,
  );
  assert(
    view.some((m) => m.content === "new session history"),
    `${label}: the new authority tail renders (${view.map((m) => m.content)})`,
  );

  // The old subscription goes silent, then answers once more for its own dead
  // binding: that observed mismatch — not a deadline — drives recovery.
  const startsBeforeStaleEvent = server.starts.length;
  await act(async () => {
    server.emit([row("ghost", "row from the dead subscription")], stale);
  });
  await flush();
  assert(
    !view.some((m) => m.content === "row from the dead subscription"),
    `${label}: a stale-binding row never reaches the transcript`,
  );
  assert(
    server.starts.length === startsBeforeStaleEvent + 1,
    `${label}: stale authority triggers exactly one recovery of the current session`,
  );
  assert(
    view.some((m) => m.content === "new session history"),
    `${label}: recovery re-adopts the current authority tail`,
  );
  await act(async () => server.emit([row("live-answer", "opaque-live-answer")], fresh));
  await flush();
  assert(view.some((message) => message.content === "opaque-live-answer"), `${label}: fresh assistant turn renders live without reload`);
  if (held) assert(view.filter((message) => message.id === "one" || message.id === "two").length === 2, `${label}: both failed rows remain visible`);
  assert(scheduled.length === 0, `${label}: recovery scheduled a timer (${scheduled.join(",")})`);

  await act(async () => root.unmount());
  host.remove();
  assert(server.live.size === 0, `${label}: unmount leaves no live subscription`);
  Object.defineProperty(globalThis, "setTimeout", { configurable: true, value: realSetTimeout });
  Object.defineProperty(globalThis, "setInterval", { configurable: true, value: realSetInterval });
  console.log(`PASS stale subscription replaced on an observed edge (${label})`);
}

async function main(): Promise<void> {
  setupHookDom();
  for (const ranged of [true, false]) for (const held of [false, true]) await runVariant(ranged, held);
  console.log("chat stale-subscription checks passed");
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
