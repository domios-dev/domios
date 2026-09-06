// A dropped delta converges on the newest server tail with no user action and
// no elapsed time, over a ranged (PTY/JSONL) and an unranged (chatBackend)
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

/** One tab of the daemon seam. `ranged` mirrors a JSONL tailer that stamps the
 *  byte range it consumed; unranged mirrors a store-backed chatBackend. */
class FakeChatServer {
  handler: ChatHandler | null = null;
  starts: string[] = [];
  offset = 100;
  transcript: NormalizedChatMessage[] = [];

  constructor(readonly ranged: boolean) {}

  install(bridge: {
    __setInvokeForTest: (fn: unknown) => void;
    __setListenForTest: (fn: unknown) => void;
  }): void {
    bridge.__setListenForTest(async (_event: string, next: ChatHandler) => {
      this.handler = next;
      return () => {
        if (this.handler === next) this.handler = null;
      };
    });
    bridge.__setInvokeForTest(async (command: string, args: Record<string, unknown>) => {
      if (command === "chat_start") {
        const subscriptionId = String(args.subscriptionId ?? "");
        this.starts.push(subscriptionId);
        return {
          text: "",
          start_offset: this.ranged ? 0 : undefined,
          offset: this.ranged ? this.offset : 0,
          pending: false,
          messages: [...this.transcript],
          source_session_id: "sid-1",
          session_binding_nonce: "nonce-1",
          subscription_id: subscriptionId,
        };
      }
      return {};
    });
  }

  /** Emit rows the server also appended to its transcript. `skipBytes` models a
   *  delta the client never received: the tail advances past the committed end. */
  emit(rows: NormalizedChatMessage[], skipBytes = 0): void {
    this.transcript = [...this.transcript, ...rows];
    const start = this.offset + skipBytes;
    const end = start + rows.length * 10;
    this.offset = end;
    this.handler?.({
      payload: {
        tab_id: "tab",
        messages: rows,
        start_offset: this.ranged ? start : undefined,
        offset: this.ranged ? end : undefined,
        source_session_id: "sid-1",
        session_binding_nonce: "nonce-1",
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

async function runVariant(ranged: boolean): Promise<void> {
  const label = ranged ? "ranged" : "unranged";
  const bridge = await import("../../lib/bridge");
  const { useChatSession } = await import("./useChatSession");
  const { clearTranscriptWindows } = await import("./transcriptStore");
  clearTranscriptWindows();

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

  const server = new FakeChatServer(ranged);
  server.transcript = [row("s-1", "history")];
  server.install(bridge as never);

  let view: NormalizedChatMessage[] = [];
  function Harness() {
    const session = useChatSession({
      tabId: "tab",
      agentSessionId: "sid-1",
      sessionBindingNonce: "nonce-1",
      active: true,
    });
    view = session.allMessages;
    return null;
  }
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root: Root = createRoot(host);
  await act(async () => {
    root.render(createElement(Harness));
  });
  await flush();
  assert(view.some((m) => m.content === "history"), `${label}: snapshot renders`);
  const startsAfterAttach = server.starts.length;

  // The client never saw the bytes between the committed end and this delta.
  await act(async () => {
    server.emit([row("live-2", "newest server row")], 500);
  });
  await flush();

  if (ranged) {
    assert(
      server.starts.length === startsAfterAttach + 1,
      `${label}: a gap requests exactly one re-attach (got ${server.starts.length - startsAfterAttach})`,
    );
  } else {
    assert(
      server.starts.length === startsAfterAttach,
      `${label}: a source with no byte model has no gap to recover from`,
    );
  }
  assert(
    view[view.length - 1]?.content === "newest server row",
    `${label}: the newest server row is the last rendered row (${view.map((m) => m.content)})`,
  );
  assert(
    scheduled.length === 0,
    `${label}: recovery scheduled a timer (${scheduled.join(",")})`,
  );

  await act(async () => root.unmount());
  host.remove();
  Object.defineProperty(globalThis, "setTimeout", { configurable: true, value: realSetTimeout });
  Object.defineProperty(globalThis, "setInterval", { configurable: true, value: realSetInterval });
  console.log(`PASS dropped delta converges (${label})`);
}

async function main(): Promise<void> {
  setupHookDom();
  await runVariant(true);
  await runVariant(false);
  console.log("chat dropped-delta convergence checks passed");
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
