// Run: npx tsx src/components/Terminal/useChatSession.unavailable.test.tsx

import { act } from "react";
import { Window } from "happy-dom";
import { createRoot } from "react-dom/client";
import { chatBodyView } from "./chatBodyVisibility";
import type { ChatUnavailableReason } from "./chatSessionAuthority";
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

interface Observed {
  loading: boolean;
  recovering: boolean;
  error: string | null;
  transcriptUnavailable: ChatUnavailableReason | null;
  messages: string[];
}

function bodyKind(observed: Observed, hasTranscript: boolean): string {
  return chatBodyView({
    hasTranscript,
    chatLoading: observed.loading || observed.recovering || observed.error != null,
    unavailable: observed.transcriptUnavailable,
    bootView: { kind: "none" },
  }).kind;
}

async function main() {
  // A terminal state that arrived on a schedule would be a deadline wearing a
  // readiness costume; nothing in this hook may schedule one.
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
  const listeners = new Map<string, (event: { payload: ChatEventPayload }) => void>();
  const snapshots = new Map<string, () => Record<string, unknown>>();
  let starts = 0;

  __setListenForTest(async (event, handler) => {
    const match = /^tab-chat-msg-(.+)$/.exec(event);
    if (match) listeners.set(match[1], handler as (e: { payload: ChatEventPayload }) => void);
    return () => {
      if (match) listeners.delete(match[1]);
    };
  });
  __setInvokeForTest((async (cmd: string, args: { tabId?: string }) => {
    if (cmd === "chat_start") {
      starts += 1;
      return snapshots.get(args.tabId ?? "")?.() ?? {};
    }
    return {};
  }) as never);

  const observed = new Map<string, Observed>();
  function Harness({ tabId, sessionId }: { tabId: string; sessionId: string }) {
    const session = useChatSession({ tabId, agentSessionId: sessionId, sessionBindingNonce: null });
    observed.set(tabId, {
      loading: session.loading,
      recovering: session.recovering,
      error: session.error,
      transcriptUnavailable: session.transcriptUnavailable,
      messages: session.allMessages.map((message) => message.content),
    });
    return null;
  }
  const readState = (tabId: string): Observed => {
    const state = observed.get(tabId);
    assert(state, `no observed state for ${tabId}`);
    return state;
  };

  // A provider that names no transcript source: the host says so, and the wait
  // that would never end is reported instead of entered.
  const missingTab = "codex-no-source";
  snapshots.set(missingTab, () => ({
    text: "",
    offset: 0,
    pending: true,
    source_missing: true,
    source_session_id: "bound-session",
    session_binding_nonce: null,
    subscription_id: "sub-missing",
  }));
  const missingHost = document.createElement("div");
  const missingRoot = createRoot(missingHost);
  await act(async () => missingRoot.render(<Harness tabId={missingTab} sessionId="bound-session" />));

  let state = readState(missingTab);
  assert(state.transcriptUnavailable === "no_transcript_source", "a sourceless bound session was not reported");
  assert(state.loading === false, "a reported failure left the hook loading");
  assert(bodyKind(state, false) === "failed", "the chat body kept spinning on a reported failure");
  assert(scheduledWaits.length === 0, "the terminal state was reached on a timer");
  assert(starts === 1, "the reported failure re-attached instead of settling");

  // Only an observed delta clears it — never elapsed time, never a re-poll.
  await act(async () => listeners.get(missingTab)?.({
    payload: {
      tab_id: missingTab,
      source_session_id: "bound-session",
      session_binding_nonce: null,
      messages: [{ id: "late", type: "assistant", content: "late transcript", timestamp: "1" }],
    },
  }));
  state = readState(missingTab);
  assert(state.transcriptUnavailable === null, "an observed delta did not clear the reported failure");
  assert(state.messages[0] === "late transcript", "the observed delta was not committed");
  assert(bodyKind(state, true) === "none", "the recovered body did not return to content");
  await act(async () => missingRoot.unmount());

  // A snapshot the tab cannot adopt while its own authority never moved: nobody
  // will re-ask, so it is reported rather than left as an open wait.
  const mismatchTab = "mismatch-tab";
  snapshots.set(mismatchTab, () => ({
    text: "",
    offset: 0,
    pending: false,
    messages: [],
    source_session_id: "some-other-session",
    session_binding_nonce: null,
    subscription_id: "sub-foreign",
  }));
  const mismatchHost = document.createElement("div");
  const mismatchRoot = createRoot(mismatchHost);
  await act(async () => mismatchRoot.render(<Harness tabId={mismatchTab} sessionId="bound-session" />));

  state = readState(mismatchTab);
  assert(state.transcriptUnavailable === "authority_mismatch", "an unadoptable snapshot was not reported");
  assert(state.loading === false, "an unadoptable snapshot left the hook loading");
  assert(bodyKind(state, false) === "failed", "the chat body kept spinning on an unadoptable snapshot");
  assert(scheduledWaits.length === 0, `the hook scheduled ${scheduledWaits.length} timed waits`);
  await act(async () => mismatchRoot.unmount());

  Object.defineProperty(globalThis, "setTimeout", { value: nativeSetTimeout, configurable: true });
  console.log("PASS the chat loader reaches a reported terminal state, never an unbounded wait");
}

void main();
