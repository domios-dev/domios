import assert from "node:assert/strict";
import { test } from "node:test";
import { act, createElement, useRef, useState } from "react";
import { flushSync } from "react-dom";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { createTranscriptState, reduceTranscript, type TranscriptState } from "@codeterm/chat-core";
import { useChatWindowView } from "./useChatWindowView";
import { clearTranscriptWindows, useTranscriptWindow } from "./transcriptStore";
import { __setChatSendRepositoryForTest, ensureChatSendTabHydrated, chatSendTabHydrationState,
  requestChatSendDismissalMarkers } from "./chatSendStore";
import { encodeChatSend, encodeChatSendDismissalMarker, encodeChatSendTombstone,
  mergeChatSendDismissalMarker } from "./chatSendDurableModel";
import { MemoryChatSendRepository } from "./chatSendRepository.testSupport";

const authority = { sourceSessionId: "session", sessionBindingNonce: "nonce" };
function page(state: TranscriptState, start: number, end: number) {
  return reduceTranscript(state, { kind: start === 220 ? "snapshot" : "backfill", authority,
    range: { start, end, unit: "items" }, rows: Array.from({ length: end - start }, (_, i) => ({
      id: `row-${start + i}`, type: "assistant" as const, content: `answer-${start + i}`,
    })) }).state;
}

test("backfill rendered before request completion still reveals the merged older window", async () => {
  const browser = new Window();
  Object.defineProperties(globalThis, {
    window: { configurable: true, value: browser }, document: { configurable: true, value: browser.document },
    navigator: { configurable: true, value: browser.navigator }, IS_REACT_ACT_ENVIRONMENT: { configurable: true, value: true },
  });
  let view: ReturnType<typeof useChatWindowView> | undefined;
  let requests = 0;
  const initial = page(createTranscriptState(), 220, 320);
  function Harness() {
    const [transcript, setTranscript] = useState(initial);
    const followTailRef = useRef(true);
    const { anchorKey } = useTranscriptWindow("reveal:session");
    view = useChatWindowView({ tabId: "reveal", windowKey: "reveal:session", transcript, anchorKey, followTailRef,
      loadOlderPage: async (cursor) => {
        requests++;
        assert.equal(cursor, 220);
        flushSync(() => setTranscript(page(initial, 0, 220)));
        return { cursor: 0, consumedBytes: 0, rows: 220 };
      },
    });
    return createElement("output", null, view.enrichedMessages[0]?.msg.id);
  }
  const host = document.createElement("main");
  const root = createRoot(host);
  try {
    await act(async () => root.render(createElement(Harness)));
    assert.equal(host.textContent, "row-220");
    await act(async () => { await view?.loadOlder(); });
    assert.equal(requests, 1);
    assert.equal(view?.allMessages.length, 320);
    assert.equal(host.textContent, "row-120");
  } finally {
    await act(async () => root.unmount());
    clearTranscriptWindows();
    await browser.happyDOM.abort();
  }
});

test("transcript history stays visible until a complete send ledger confirms dismissal", async () => {
  const browser = new Window();
  Object.defineProperties(globalThis, {
    window: { configurable: true, value: browser }, document: { configurable: true, value: browser.document },
    navigator: { configurable: true, value: browser.navigator }, IS_REACT_ACT_ENVIRONMENT: { configurable: true, value: true },
  });
  const tabId = "reveal-during-send-hydration";
  const sendId = "owned-echo";
  let finishHydration!: () => void;
  const repository = new MemoryChatSendRepository();
  repository.hydrationGate = new Promise<void>((resolve) => { finishHydration = resolve; });
  repository.records.set(JSON.stringify([tabId, sendId]), encodeChatSend(tabId, {
    id: sendId, correlationId: sendId, content: "owned text", sentAtMs: 1, dismissed: true,
  }));
  __setChatSendRepositoryForTest(repository);
  const transcript = reduceTranscript(createTranscriptState(), {
    kind: "snapshot", authority, range: { start: 0, end: 1, unit: "items" },
    rows: [{ id: sendId, type: "user", content: "owned text", correlationId: sendId, timestamp: "1" }],
  }).state;
  let view: ReturnType<typeof useChatWindowView> | undefined;
  function Harness() {
    const followTailRef = useRef(true);
    const { anchorKey } = useTranscriptWindow(`${tabId}:session`);
    view = useChatWindowView({ tabId, windowKey: `${tabId}:session`, transcript, anchorKey, followTailRef,
      loadOlderPage: async () => null,
    });
    return createElement("output", null, view.allMessages.map((message) => message.id).join(","));
  }
  const host = document.createElement("main");
  const root = createRoot(host);
  try {
    await act(async () => root.render(createElement(Harness)));
    assert.equal(chatSendTabHydrationState(tabId), "pending");
    assert.equal(host.textContent, sendId, "incomplete persistence cannot remove an authoritative transcript row");

    await act(async () => {
      finishHydration();
      await ensureChatSendTabHydrated(tabId);
    });
    assert.equal(chatSendTabHydrationState(tabId), "ready");
    assert.equal(host.textContent, "", "the complete persisted dismissal is applied to the projection");
  } finally {
    finishHydration();
    await act(async () => root.unmount());
    clearTranscriptWindows();
    __setChatSendRepositoryForTest(null);
    await browser.happyDOM.abort();
  }
});

test("failed send hydration leaves the authoritative transcript projection intact", async () => {
  const browser = new Window();
  Object.defineProperties(globalThis, {
    window: { configurable: true, value: browser }, document: { configurable: true, value: browser.document },
    navigator: { configurable: true, value: browser.navigator }, IS_REACT_ACT_ENVIRONMENT: { configurable: true, value: true },
  });
  const tabId = "reveal-after-send-storage-error";
  let finishHydration!: () => void;
  const repository = new MemoryChatSendRepository();
  repository.hydrationGate = new Promise<void>((resolve) => { finishHydration = resolve; });
  repository.failHydration = true;
  __setChatSendRepositoryForTest(repository);
  const transcript = reduceTranscript(createTranscriptState(), {
    kind: "snapshot", authority, range: { start: 0, end: 1, unit: "items" },
    rows: [{ id: "still-authoritative", type: "user", content: "owned text", correlationId: "still-authoritative", timestamp: "1" }],
  }).state;
  let view: ReturnType<typeof useChatWindowView> | undefined;
  function Harness() {
    const followTailRef = useRef(true);
    const { anchorKey } = useTranscriptWindow(`${tabId}:session`);
    view = useChatWindowView({ tabId, windowKey: `${tabId}:session`, transcript, anchorKey, followTailRef,
      loadOlderPage: async () => null,
    });
    return createElement("output", null, view.allMessages.map((message) => message.id).join(","));
  }
  const host = document.createElement("main");
  const root = createRoot(host);
  try {
    await act(async () => root.render(createElement(Harness)));
    assert.equal(host.textContent, "still-authoritative", "transcript rows render while send persistence is pending");
    finishHydration();
    await act(async () => { await ensureChatSendTabHydrated(tabId); });
    assert.equal(chatSendTabHydrationState(tabId), "error");
    assert.equal(host.textContent, "still-authoritative", "storage failure is not interpreted as an empty dismissal ledger");
  } finally {
    finishHydration();
    await act(async () => root.unmount());
    clearTranscriptWindows();
    __setChatSendRepositoryForTest(null);
    await browser.happyDOM.abort();
  }
});

test("compacted dismissal marker still hides a historical matched echo", async () => {
  const browser = new Window();
  Object.defineProperties(globalThis, {
    window: { configurable: true, value: browser }, document: { configurable: true, value: browser.document },
    navigator: { configurable: true, value: browser.navigator }, IS_REACT_ACT_ENVIRONMENT: { configurable: true, value: true },
  });
  const tabId = "reveal-compacted-dismissal";
  const repository = new MemoryChatSendRepository();
  const matchedRow = { source: { source_session_id: "session", record: {
    kind: "structured" as const, thread_id: "session", item_id: "provider-item",
  } }, ordinal: 4 };
  const dismissed = { id: "old-send", correlationId: "old-correlation", content: "large historical payload",
    sentAtMs: 10, dismissed: true, matchedRow, echo: { content: "historical message", timestampMs: 10_000 } };
  repository.records.set(JSON.stringify([tabId, dismissed.id]), encodeChatSendTombstone(tabId, dismissed.id, 2, 4));
  const priorMarker = encodeChatSendDismissalMarker(tabId, {
    id: dismissed.id, correlationId: dismissed.correlationId, content: dismissed.content,
    sentAtMs: dismissed.sentAtMs, dismissed: true, echo: dismissed.echo,
  }, 0, 4);
  repository.dismissals.set(JSON.stringify([tabId, dismissed.id]), mergeChatSendDismissalMarker(
    priorMarker, encodeChatSendDismissalMarker(tabId, dismissed, 1, 4),
  ));
  const echoOnly = { id: "echo-send", content: "echo text", sentAtMs: 22, dismissed: true,
    echo: { content: "echo text", timestampMs: 22_000 } };
  repository.records.set(JSON.stringify([tabId, echoOnly.id]), encodeChatSendTombstone(tabId, echoOnly.id, 2, 5));
  repository.dismissals.set(JSON.stringify([tabId, echoOnly.id]),
    encodeChatSendDismissalMarker(tabId, echoOnly, 1, 5));
  __setChatSendRepositoryForTest(repository);
  const transcript = reduceTranscript(createTranscriptState(), {
    kind: "snapshot", authority, range: { start: 0, end: 3, unit: "items" },
    rows: [
      { id: "history-row", sourceMessageId: "provider-item", sourceSessionId: "session",
        type: "user", content: "historical message", timestamp: "10" },
      { id: "echo-row", type: "user", content: "echo   text", timestamp: "22" },
      { id: "echo-decoy", sourceMessageId: "different-provider-item", sourceSessionId: "session",
        type: "user", content: "historical message", timestamp: "10" },
    ],
  }).state;
  let view: ReturnType<typeof useChatWindowView> | undefined;
  function Harness() {
    const followTailRef = useRef(true);
    const { anchorKey } = useTranscriptWindow(`${tabId}:session`);
    view = useChatWindowView({ tabId, windowKey: `${tabId}:session`, transcript, anchorKey, followTailRef,
      loadOlderPage: async () => null,
    });
    return createElement("output", null, view.allMessages.map((message) => message.id).join(","));
  }
  const host = document.createElement("main");
  const root = createRoot(host);
  try {
    await act(async () => root.render(createElement(Harness)));
    await act(async () => {
      await ensureChatSendTabHydrated(tabId);
      await requestChatSendDismissalMarkers(tabId, view!.committedRows);
    });

    assert.equal(chatSendTabHydrationState(tabId), "ready");
    assert.deepEqual(view?.allMessages.map((message) => message.id), ["echo-decoy"]);
    assert.equal(host.textContent, "echo-decoy");
    assert.equal(JSON.stringify(repository.dismissals.get(JSON.stringify([tabId, dismissed.id]))).includes("large historical payload"), false);
  } finally {
    await act(async () => root.unmount());
    clearTranscriptWindows();
    __setChatSendRepositoryForTest(null);
    await browser.happyDOM.abort();
  }
});
