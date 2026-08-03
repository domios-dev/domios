// Run: npx tsx src/components/Terminal/useChatSession.reactivate.test.ts
// Regression probe for the tab-return live-update seam: leaving a Chat tab
// (active=false) and returning (active=true) must reattach exactly once and
// resume live deltas without a Terminal<->Chat remount. Each block drives one
// adversarial ordering of listen/chat_start/delta/stop.
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

interface ServerLog {
  starts: Array<{ subscriptionId: string }>;
  stops: Array<{ subscriptionId: string }>;
  listens: number;
  unlistens: number;
}

let fakeServerSequence = 0;

/** Minimal fake of the daemon seam: registry semantics for one tab. */
class FakeChatServer {
  /** Distinct per instance: the projection cache is keyed by tab+session and
   *  must not leak one scenario's committed byte range into the next. */
  tabId = `tab-${(fakeServerSequence += 1)}`;
  log: ServerLog = { starts: [], stops: [], listens: 0, unlistens: 0 };
  handler: ChatHandler | null = null;
  sessionId = "sid-1";
  snapshot: NormalizedChatMessage[] = [];
  /** Byte range of the parsed snapshot window; live deltas extend it. */
  snapshotStart = 0;
  offset = 100;
  subscribers = new Set<string>();
  startGate: (() => Promise<void>) | null = null;

  install(bridge: {
    __setInvokeForTest: (fn: unknown) => void;
    __setListenForTest: (fn: unknown) => void;
  }): void {
    bridge.__setListenForTest(async (_event: string, nextHandler: ChatHandler) => {
      this.log.listens += 1;
      this.handler = nextHandler;
      return () => {
        this.log.unlistens += 1;
        if (this.handler === nextHandler) this.handler = null;
      };
    });
    bridge.__setInvokeForTest(async (command: string, args: Record<string, unknown>) => {
      if (command === "chat_start") {
        const subscriptionId = String(args.subscriptionId ?? "");
        this.log.starts.push({ subscriptionId });
        // The window is parsed before the response travels, exactly like the
        // daemon: a delta racing the reply lands beyond this range.
        const parsed = {
          text: "",
          start_offset: this.snapshotStart,
          offset: this.offset,
          pending: false,
          messages: [...this.snapshot],
          source_session_id: this.sessionId,
          session_binding_nonce: "nonce-1",
          subscription_id: subscriptionId,
        };
        if (this.startGate) await this.startGate();
        this.subscribers.add(subscriptionId);
        return parsed;
      }
      if (command === "chat_stop") {
        const subscriptionId = String(args.subscriptionId ?? "");
        this.log.stops.push({ subscriptionId });
        this.subscribers.delete(subscriptionId);
        return {};
      }
      return {};
    });
  }

  /** Emit a live delta exactly like the daemon sink: tab event with authority
   *  stamp and the exact byte range the tailer consumed. */
  emit(
    messages: NormalizedChatMessage[],
    sourceSessionId: string | null = this.sessionId,
    span = 20,
  ): void {
    const start = this.offset;
    this.offset = start + span;
    this.handler?.({
      payload: {
        tab_id: this.tabId,
        messages,
        start_offset: start,
        offset: this.offset,
        source_session_id: sourceSessionId,
        session_binding_nonce: "nonce-1",
      },
    });
  }

  tailing(): boolean {
    return this.subscribers.size > 0;
  }
}

function msg(id: string, content: string, ts: string): NormalizedChatMessage {
  return { id, type: "assistant", content, timestamp: ts };
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
  render: (sessionId: string | null, active: boolean) => Promise<void>;
  messages: () => NormalizedChatMessage[];
}

async function main(): Promise<void> {
  setupHookDom();
  const bridge = await import("../../lib/bridge");
  const { useChatSession } = await import("./useChatSession");

  let view: NormalizedChatMessage[] = [];
  async function mount(server: FakeChatServer): Promise<Mounted> {
    server.install(bridge as never);
    function Harness({ sid, active }: { sid: string | null; active: boolean }) {
      const session = useChatSession({
        tabId: server.tabId,
        agentSessionId: sid,
        sessionBindingNonce: "nonce-1",
        active,
      });
      view = session.allMessages;
      return null;
    }
    const host = document.createElement("div");
    document.body.appendChild(host);
    const root = createRoot(host);
    const render = async (sid: string | null, active: boolean) => {
      await act(async () => {
        root.render(createElement(Harness, { sid, active }));
      });
      await flush();
    };
    await render(server.sessionId, true);
    return { root, render, messages: () => view };
  }

  // --- 1. plain leave/return: exactly one reattach, live deltas resume ---
  {
    const server = new FakeChatServer();
    server.snapshot = [msg("m1", "first", "1")];
    const app = await mount(server);
    const initialStarts: number = server.log.starts.length;
    assert(initialStarts === 1, "initial activation starts once");
    server.emit([msg("m2", "live-1", "2")]);
    await flush();
    assert(app.messages().some((m) => m.content === "live-1"), "live delta lands while active");

    await app.render(server.sessionId, false);
    assert(!server.tailing(), "hidden tab must not keep a subscription (no background tail)");
    assert(server.handler === null, "hidden tab detaches its event listener");

    server.snapshot = [msg("m1", "first", "1"), msg("m2", "live-1", "2"), msg("m3", "while-hidden", "3")];
    await app.render(server.sessionId, true);
    const startsAfterReturn: number = server.log.starts.length;
    assert(startsAfterReturn === 2, `return reattaches exactly once (starts=${startsAfterReturn})`);
    assert(
      app.messages().some((m) => m.content === "while-hidden"),
      "accumulated deltas render immediately from the reattach snapshot",
    );
    server.emit([msg("m4", "live-2", "4")]);
    await flush();
    assert(
      app.messages().some((m) => m.content === "live-2"),
      "live deltas resume after returning to the tab (no view toggle needed)",
    );
    await act(async () => app.root.unmount());
    assert(!server.tailing(), "unmount releases the subscription");
  }

  // --- 2. delta arrives between listener attach and chat_start resolution ---
  {
    const server = new FakeChatServer();
    server.snapshot = [msg("m1", "first", "1")];
    const app = await mount(server);
    await app.render(server.sessionId, false);

    let releaseStart: (() => void) | null = null;
    server.startGate = () =>
      new Promise<void>((resolve) => {
        releaseStart = resolve;
      });
    server.snapshot = [msg("m1", "first", "1"), msg("m2", "while-hidden", "2")];
    // Reactivation completes its render with chat_start still hanging at the gate.
    await app.render(server.sessionId, true);
    assert(server.handler !== null, "harness: reattach listener registered before the racing delta");
    assert(server.log.starts.length >= 2, "harness: reattach chat_start already issued");
    // Live delta races ahead of the chat_start response.
    await act(async () => {
      server.emit([msg("m3", "early-live", "3")]);
    });
    await act(async () => {
      releaseStart?.();
      server.startGate = null;
    });
    await flush();
    assert(
      app.messages().some((m) => m.content === "early-live"),
      "a delta racing ahead of the reattach snapshot is not dropped",
    );
    assert(
      app.messages().some((m) => m.content === "while-hidden"),
      "the reattach snapshot still lands after the racing delta",
    );
    await act(async () => app.root.unmount());
  }

  // --- 3. leave while chat_start is in flight; stale result must not leak a subscription ---
  {
    const server = new FakeChatServer();
    server.snapshot = [msg("m1", "first", "1")];
    const app = await mount(server);
    await app.render(server.sessionId, false);

    const gate: { release: (() => void) | null } = { release: null };
    server.startGate = () =>
      new Promise<void>((resolve) => {
        gate.release = resolve;
      });
    // Return, then leave again before chat_start resolves.
    await act(async () => {
      const p = app.render(server.sessionId, true);
      await Promise.resolve();
      await p;
    });
    await app.render(server.sessionId, false);
    gate.release?.();
    server.startGate = null;
    await flush();
    assert(
      !server.tailing(),
      `a chat_start resolving after deactivation must be stopped server-side (subs=${[...server.subscribers]})`,
    );

    // Final return still works.
    server.snapshot = [msg("m1", "first", "1"), msg("m2", "late", "2")];
    await app.render(server.sessionId, true);
    server.emit([msg("m3", "live-after", "3")]);
    await flush();
    assert(
      app.messages().some((m) => m.content === "live-after"),
      "live deltas resume after an interrupted reattach cycle",
    );
    await act(async () => app.root.unmount());
  }

  // --- 4. session rotates while hidden: return binds the NEW session's stream ---
  {
    const server = new FakeChatServer();
    server.snapshot = [msg("m1", "old-history", "1")];
    const app = await mount(server);
    await app.render(server.sessionId, false);

    server.sessionId = "sid-2";
    server.snapshot = [msg("n1", "new-history", "5")];
    await app.render("sid-2", true);
    assert(
      app.messages().some((m) => m.content === "new-history"),
      "return after rotation renders the new session snapshot",
    );
    server.emit([msg("n2", "new-live", "6")], "sid-2");
    await flush();
    assert(
      app.messages().some((m) => m.content === "new-live"),
      "live deltas from the rotated session are adopted after return",
    );
    await act(async () => app.root.unmount());
  }

  // --- 5. an advanced bounded window with reused window-local seq keeps the
  // prior completed turn and adopts the new tail instead of replacing rows ---
  {
    const server = new FakeChatServer();
    server.snapshotStart = 1_000;
    server.offset = 1_400;
    server.snapshot = [
      { ...msg("0-0", "older question", "1"), timestamp: undefined, seq: 0 },
      { ...msg("1-0", "prior answer", "2"), timestamp: undefined, seq: 1 },
    ];
    const app = await mount(server);
    await app.render(server.sessionId, false);

    // The source window advanced: same session, new start offset, and the
    // parser restarted its window-local ids and seq from zero.
    server.snapshotStart = 1_200;
    server.offset = 1_800;
    server.snapshot = [
      { ...msg("0-0", "prior answer", "3"), timestamp: undefined, seq: 0 },
      { ...msg("1-0", "new tool run", "4"), timestamp: undefined, seq: 1 },
    ];
    await app.render(server.sessionId, true);
    assert(
      app.messages().map((message) => message.content).join(",") === "prior answer,new tool run",
      `advanced window keeps the prior turn and appends the tail (${app.messages().map((m) => m.content)})`,
    );
    await act(async () => app.root.unmount());
  }

  // --- 6. reattach + live/send race cannot project a duplicate row, and does
  // not collapse genuinely repeated authored content ---
  {
    const server = new FakeChatServer();
    server.snapshotStart = 0;
    server.offset = 300;
    server.snapshot = [
      { ...msg("0-0", "continue", "1"), type: "user", timestamp: undefined, seq: 0 },
      { ...msg("1-0", "continue", "2"), type: "user", timestamp: undefined, seq: 1 },
      { ...msg("2-0", "on it", "3"), timestamp: undefined, seq: 2 },
    ];
    const app = await mount(server);
    server.emit([{ ...msg("300-0", "live answer", "4"), timestamp: undefined, seq: 0 }]);
    await flush();
    await app.render(server.sessionId, false);

    // Reattach: the transport mints new ids for the same bytes and reuses seq.
    server.snapshot = [
      { ...msg("a-0", "continue", "1"), type: "user", timestamp: undefined, seq: 0 },
      { ...msg("a-1", "continue", "2"), type: "user", timestamp: undefined, seq: 1 },
      { ...msg("a-2", "on it", "3"), timestamp: undefined, seq: 2 },
      { ...msg("a-3", "live answer", "4"), timestamp: undefined, seq: 3 },
    ];
    await app.render(server.sessionId, true);
    const projected = app.messages().map((message) => message.content);
    assert(
      projected.join(",") === "continue,continue,on it,live answer",
      `reattachment projects each row once and keeps real repeats (${projected})`,
    );

    // A new send of identical text is a real new row, not a duplicate to drop.
    server.emit([{ ...msg("320-0", "continue", "5"), type: "user", timestamp: undefined, seq: 0 }]);
    await flush();
    assert(
      app.messages().map((message) => message.content).join(",")
        === "continue,continue,on it,live answer,continue",
      "a genuinely repeated send is adopted after the committed turn",
    );
    await act(async () => app.root.unmount());
  }

  console.log("chat reactivation lifecycle checks passed");
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
