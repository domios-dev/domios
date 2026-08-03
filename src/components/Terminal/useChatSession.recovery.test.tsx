// Run: npx tsx src/components/Terminal/useChatSession.recovery.test.tsx

import { act } from "react";
import { Window } from "happy-dom";
import { createRoot } from "react-dom/client";
import type { ChatAttachmentState } from "./chatSessionAuthority";
import { useChatSession } from "./useChatSession";

const window = new Window({ url: "http://localhost/" });
Object.defineProperty(globalThis, "IS_REACT_ACT_ENVIRONMENT", { value: true, configurable: true });
Object.defineProperty(globalThis, "window", { value: window, configurable: true });
Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
Object.defineProperty(globalThis, "navigator", { value: window.navigator, configurable: true });
Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
Object.defineProperty(globalThis, "Event", { value: window.Event, configurable: true });
Object.defineProperty(globalThis, "CustomEvent", { value: window.CustomEvent, configurable: true });

function assert(condition: unknown, label: string): asserts condition {
  if (!condition) throw new Error(label);
}

interface ChatEventPayload {
  tab_id: string;
  source_session_id: string | null;
  session_binding_nonce: string | null;
  messages: Array<{ id: string; type: "assistant"; content: string; timestamp: string }>;
}

async function main() {
  // Attach readiness is proven by an observed event or attach edge, never by a
  // deadline, so any scheduled wait is itself the contract violation.
  const scheduledWaits: number[] = [];
  const nativeSetTimeout = globalThis.setTimeout;
  Object.defineProperty(globalThis, "setTimeout", {
    value: ((callback: () => void, delay?: number) => {
      if ((delay ?? 0) >= 1_000) scheduledWaits.push(delay ?? 0);
      return nativeSetTimeout(callback, delay);
    }) as typeof setTimeout,
    configurable: true,
  });
  const { __setInvokeForTest, __setListenForTest } = await import("../../lib/bridge");
  let starts = 0;
  let stops = 0;
  let listener: ((event: { payload: ChatEventPayload }) => void) | null = null;
  __setListenForTest(async (_event, handler) => {
    listener = handler as typeof listener;
    return () => {};
  });
  __setInvokeForTest((async (cmd: string, args: { agentSessionId?: string | null }) => {
    if (cmd === "chat_start") {
      starts += 1;
      const sourceSessionId = args.agentSessionId ?? null;
      return {
        text: "",
        offset: 0,
        pending: true,
        source_session_id: sourceSessionId,
        session_binding_nonce: null,
        subscription_id: sourceSessionId == null ? null : `sub-${starts}`,
      };
    }
    if (cmd === "chat_stop") stops += 1;
    return {};
  }) as never);

  let latest: {
    attachmentState: ChatAttachmentState;
    error: string | null;
    recovering: boolean;
    messages: string[];
    retry: () => void;
  } = {
    attachmentState: "idle",
    error: null as string | null,
    recovering: false,
    messages: [] as string[],
    retry: () => {},
  };
  function Harness({ sessionId }: { sessionId: string | null }) {
    const session = useChatSession({ tabId: "tab", agentSessionId: sessionId, sessionBindingNonce: null });
    latest = {
      attachmentState: session.attachmentState,
      error: session.error,
      recovering: session.recovering,
      messages: session.allMessages.map((message) => message.content),
      retry: session.retry,
    };
    return null;
  }
  const attachmentState = (): ChatAttachmentState => latest.attachmentState;

  const deliver = async (id: string, content: string) => {
    await act(async () => listener?.({
      payload: {
        tab_id: "tab",
        source_session_id: "bound-session",
        session_binding_nonce: null,
        messages: [{ id, type: "assistant", content, timestamp: id }],
      },
    }));
  };

  const host = document.createElement("div");
  const root = createRoot(host);
  await act(async () => root.render(<Harness sessionId={null} />));
  assert(starts === 1, "fresh snapshot started more than once");
  assert(attachmentState() === "awaiting_session", "fresh chat did not await its first session");
  assert(!latest.recovering, "fresh chat was misclassified as recovering history");
  assert(latest.error == null, "fresh chat surfaced a history error");
  assert(scheduledWaits.length === 0, "awaiting a first session scheduled a wait");

  await act(async () => root.render(<Harness sessionId="bound-session" />));
  assert(Number(starts) === 2, "bound snapshot did not start exactly once");
  assert(stops === 0, "pending snapshot stopped the listener");
  assert(attachmentState() === "awaiting_store", "bound pending snapshot did not await its store");
  assert(latest.recovering, "a bound chat awaiting its store is recovering history");
  assert(scheduledWaits.length === 0, "a pending attach scheduled a wait");

  await deliver("history", "history restored");
  assert(!latest.recovering, "chat stayed in recovering state");
  assert(attachmentState() === "attached", "an observed event did not complete the attach");
  assert(latest.messages[0] === "history restored", "history was not restored");
  assert(latest.error == null, "a completed attach surfaced an error");

  await act(async () => latest.retry());
  assert(Number(starts) === 3, "retry did not request a fresh snapshot");
  assert(Number(stops) === 1, "retry did not stop exactly the prior subscription");
  assert(attachmentState() === "awaiting_store", "retry did not re-enter the awaiting state");
  assert(latest.recovering, "a re-attaching bound chat is recovering history");
  assert(scheduledWaits.length === 0, "a re-attach scheduled a failure deadline");

  // The only thing that resolves the pending re-attach is the next observed
  // event; nothing here can be advanced by elapsed time.
  await deliver("tail", "tail restored");
  assert(attachmentState() === "attached", "the observed event did not resolve the re-attach");
  assert(!latest.recovering, "the re-attached chat stayed recovering");
  assert(latest.error == null, "the re-attached chat surfaced an error");
  assert(
    latest.messages.join(",") === "history restored,tail restored",
    `re-attach lost or duplicated transcript rows (${latest.messages})`,
  );

  await act(async () => root.unmount());
  assert(Number(stops) === 2, "unmount did not release the live subscription");
  assert(scheduledWaits.length === 0, `attach scheduled ${scheduledWaits.length} timed waits`);
  Object.defineProperty(globalThis, "setTimeout", {
    value: nativeSetTimeout,
    configurable: true,
  });
  console.log("PASS attach readiness comes from observed events, never from a deadline");
}

void main();
