// Run: npx tsx src/components/Terminal/useChatSession.recovery.test.tsx

import { act } from "react";
import { Window } from "happy-dom";
import { createRoot } from "react-dom/client";
import { useChatSession } from "./useChatSession";

const window = new Window({ url: "http://localhost/" });
Object.defineProperty(globalThis, "IS_REACT_ACT_ENVIRONMENT", { value: true, configurable: true });
Object.defineProperty(globalThis, "window", { value: window, configurable: true });
Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
Object.defineProperty(globalThis, "navigator", { value: window.navigator, configurable: true });
Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
Object.defineProperty(globalThis, "Event", { value: window.Event, configurable: true });
Object.defineProperty(globalThis, "CustomEvent", { value: window.CustomEvent, configurable: true });

async function main() {
  let recoveryDeadline: (() => void) | null = null;
  const nativeSetTimeout = globalThis.setTimeout;
  Object.defineProperty(globalThis, "setTimeout", {
    value: ((callback: () => void, delay?: number) => {
      if (delay === 15_000) {
        recoveryDeadline = callback;
        return 1;
      }
      return nativeSetTimeout(callback, delay);
    }) as typeof setTimeout,
    configurable: true,
  });
  const { __setInvokeForTest, __setListenForTest } = await import("../../lib/bridge");
  let starts = 0;
  let stops = 0;
  let listener: ((event: { payload: { tab_id: string; source_session_id: string | null; session_binding_nonce: string | null; messages: Array<{ id: string; type: "assistant"; content: string; timestamp: string }> } }) => void) | null = null;
  __setListenForTest(async (_event, handler) => {
    listener = handler as typeof listener;
    return () => {};
  });
  __setInvokeForTest((async (cmd: string) => {
    if (cmd === "chat_start") {
      starts += 1;
      return {
        text: "",
        offset: 0,
        pending: true,
        source_session_id: "bound-session",
        session_binding_nonce: null,
        subscription_id: "test-subscription",
      };
    }
    if (cmd === "chat_stop") stops += 1;
    return {};
  }) as never);

  let latest = {
    attachmentState: "idle",
    error: null as string | null,
    recovering: false,
    messages: [] as string[],
    retry: () => {},
  };
  function Harness() {
    const session = useChatSession({ tabId: "tab", agentSessionId: "bound-session", sessionBindingNonce: null });
    latest = {
      attachmentState: session.attachmentState,
      error: session.error,
      recovering: session.recovering,
      messages: session.allMessages.map((message) => message.content),
      retry: session.retry,
    };
    return null;
  }

  const host = document.createElement("div");
  const root = createRoot(host);
  await act(async () => root.render(<Harness />));
  if (starts !== 1) throw new Error("pending snapshot started more than once");
  if (stops !== 0) throw new Error("pending snapshot stopped the listener");
  await act(async () => listener?.({
    payload: {
      tab_id: "tab",
      source_session_id: "bound-session",
      session_binding_nonce: null,
      messages: [{ id: "history", type: "assistant", content: "history restored", timestamp: "1" }],
    },
  }));
  if (latest.recovering) throw new Error("chat stayed in recovering state");
  if (latest.messages[0] !== "history restored") throw new Error("history was not restored");

  await act(async () => latest.retry());
  if (Number(starts) !== 2) throw new Error("retry did not request a fresh snapshot");
  if (!recoveryDeadline) throw new Error("pending recovery did not arm a failure deadline");
  await act(async () => recoveryDeadline?.());
  if (latest.attachmentState !== "failed") throw new Error("deadline did not fail the attachment");
  if (latest.recovering) throw new Error("deadline left the chat recovering");
  if (!latest.error?.includes("timed out")) throw new Error("deadline did not surface a retryable error");

  await act(async () => root.unmount());
  Object.defineProperty(globalThis, "setTimeout", {
    value: nativeSetTimeout,
    configurable: true,
  });
  console.log("PASS pending recovery restores on event and fails on a bounded deadline");
}

void main();
