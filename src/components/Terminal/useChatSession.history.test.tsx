import assert from "node:assert/strict";
import { test } from "node:test";
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import type { NormalizedChatMessage } from "@codeterm/chat-core";
import { __setInvokeForTest, __setListenForTest } from "../../lib/bridge";
import { clearTranscriptWindows } from "./transcriptStore";
import { useChatSession } from "./useChatSession";

const rows = (start: number, end: number): NormalizedChatMessage[] => Array.from({ length: end - start }, (_, index) => ({
  id: `item-${start + index}`, type: "assistant", content: `answer-${start + index}`,
}));

test("empty consumed backfill page advances to an older visible page", async () => {
  const browser = new Window();
  Object.defineProperties(globalThis, {
    window: { configurable: true, value: browser },
    document: { configurable: true, value: browser.document },
    navigator: { configurable: true, value: browser.navigator },
    IS_REACT_ACT_ENVIRONMENT: { configurable: true, value: true },
  });
  clearTranscriptWindows();
  const requests: number[] = [];
  const authority = { source_session_id: "session", session_binding_nonce: "nonce" };
  __setListenForTest(async () => () => {});
  __setInvokeForTest((async (command: string, args?: Record<string, unknown>) => {
    if (command === "chat_start") return { ...authority, text: "", pending: false,
      start_offset: 200, offset: 300, messages: rows(200, 201), subscription_id: args?.subscriptionId };
    if (command === "chat_backfill") {
      requests.push(Number(args?.beforeOffset));
      return Number(args?.beforeOffset) === 200
        ? { ...authority, start_offset: 100, end_offset: 200, messages: [] }
        : { ...authority, start_offset: 0, end_offset: 100, messages: rows(0, 1) };
    }
    return [];
  }) as Parameters<typeof __setInvokeForTest>[0]);
  let chat: ReturnType<typeof useChatSession> | undefined;
  function Harness() {
    chat = useChatSession({ tabId: "history", agentSessionId: "session", sessionBindingNonce: "nonce" });
    return createElement("output", null, chat.allMessages.map((row) => row.content).join(","));
  }
  const host = document.createElement("main");
  const root = createRoot(host);
  try {
    await act(async () => root.render(createElement(Harness)));
    await act(async () => { await chat?.loadOlder(); });
    assert.equal(chat?.canBackfillOlder, true);
    assert.equal(host.textContent, "answer-200");
    await act(async () => { await chat?.loadOlder(); });
    assert.deepEqual(requests, [200, 100]);
    assert.equal(host.textContent, "answer-0,answer-200");
    assert.equal(chat?.canBackfillOlder, false);
  } finally {
    await act(async () => root.unmount());
    clearTranscriptWindows();
    __setInvokeForTest(null);
    __setListenForTest(null);
    await browser.happyDOM.abort();
  }
});

test("recovery snapshot keeps the detached reader's loaded row visible", async () => {
  const browser = new Window();
  Object.defineProperties(globalThis, {
    window: { configurable: true, value: browser },
    document: { configurable: true, value: browser.document },
    navigator: { configurable: true, value: browser.navigator },
    IS_REACT_ACT_ENVIRONMENT: { configurable: true, value: true },
  });
  clearTranscriptWindows();
  const authority = { source_session_id: "session", session_binding_nonce: "nonce" };
  let starts = 0;
  __setListenForTest(async () => () => {});
  __setInvokeForTest((async (command: string, args?: Record<string, unknown>) => {
    if (command === "chat_start") {
      starts++;
      return { ...authority, text: "", pending: false, start_offset: 200, offset: 400,
        messages: rows(200, 400), subscription_id: args?.subscriptionId };
    }
    if (command === "chat_backfill") return { ...authority, start_offset: 0, end_offset: 200, messages: rows(0, 200) };
    return [];
  }) as Parameters<typeof __setInvokeForTest>[0]);
  let chat: ReturnType<typeof useChatSession> | undefined;
  function Harness() {
    chat = useChatSession({ tabId: "reader", agentSessionId: "session", sessionBindingNonce: "nonce" });
    return createElement("output", null, chat.enrichedMessages.map((row) => row.msg.content).join(","));
  }
  const host = document.createElement("main");
  const root = createRoot(host);
  try {
    await act(async () => root.render(createElement(Harness)));
    await act(async () => { await chat?.loadOlder(); });
    await act(async () => { assert.equal(chat?.prepareMessage("item-50"), true); });
    const detached = host.textContent;
    assert.ok(detached?.includes("answer-50"));
    await act(async () => chat?.retry());
    assert.equal(starts, 2);
    assert.equal(chat?.allMessages.length, 400);
    assert.equal(host.textContent, detached);
  } finally {
    await act(async () => root.unmount());
    clearTranscriptWindows();
    __setInvokeForTest(null);
    __setListenForTest(null);
    await browser.happyDOM.abort();
  }
});
