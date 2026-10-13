import assert from "node:assert/strict";
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { __setInvokeForTest, __setListenForTest } from "../../lib/bridge";
import { clearTranscriptWindows } from "./transcriptStore";
import { useChatSession } from "./useChatSession";
import type { NormalizedChatMessage } from "@codeterm/chat-core";
import type { ChatMsgEvent } from "./chatSessionAuthority";

const browser = new Window();
Object.defineProperties(globalThis, {
  window: { configurable: true, value: browser },
  document: { configurable: true, value: browser.document },
  navigator: { configurable: true, value: browser.navigator },
  IS_REACT_ACT_ENVIRONMENT: { configurable: true, value: true },
});

async function main() {
  let handler: ((event: { payload: ChatMsgEvent }) => void) | undefined;
  const authority = { source_session_id: "thread", session_binding_nonce: "nonce" };
  const row = { id: "assistant-i", type: "assistant" as const, content: "answer", seq: 1 };
  let snapshot: NormalizedChatMessage[] = [row];
  let start = 0;
  let end = 1;
  __setListenForTest(async (name, callback) => {
    if (name === "tab-chat-msg-rpc") handler = callback as typeof handler;
    return () => {};
  });
  __setInvokeForTest((async (command: string, args?: Record<string, unknown>) => {
    if (command === "chat_send_status") return [];
    if (command === "chat_start") return { ...authority, text: "", start_offset: start, offset: end,
      offset_kind: "items", pending: false, messages: snapshot, subscription_id: args?.subscriptionId };
    if (command === "chat_backfill") {
      assert.equal(args?.beforeOffset, 1);
      return { ...authority, text: "", start_offset: 0, end_offset: 1,
        offset_kind: "items", messages: [{ ...row, id: "older-i", content: "old", seq: 0 }] };
    }
    return {};
  }) as Parameters<typeof __setInvokeForTest>[0]);
  const host = document.createElement("main");
  document.body.append(host);
  let loadOlder: (() => Promise<unknown>) | undefined;
  function Harness() {
    const chat = useChatSession({ tabId: "rpc", agentSessionId: "thread", sessionBindingNonce: "nonce" });
    loadOlder = chat.loadOlder;
    return createElement("output", null, chat.allMessages.map(({ id, content }) =>
      createElement("p", { key: id, "data-row-id": id }, content)));
  }
  let root = createRoot(host);
  const rendered = () => Array.from(host.querySelectorAll("p"), (element) => ({
    id: element.getAttribute("data-row-id"), content: element.textContent,
  }));
  try {
    await act(async () => root.render(createElement(Harness)));
    await act(async () => handler?.({ payload: { ...authority, tab_id: "rpc", messages: [row] } }));
    assert.deepEqual(rendered(), [{ id: row.id, content: "answer" }], "snapshot and completion share one DOM row");
    await act(async () => handler?.({ payload: { ...authority, tab_id: "rpc", messages: [{ ...row, content: "answer grows" }] } }));
    assert.deepEqual(rendered(), [{ id: row.id, content: "answer grows" }]);
    await act(async () => handler?.({ payload: { ...authority, tab_id: "rpc", messages: [
      { ...row, id: "different-1", content: "equal" }, { ...row, id: "different-2", content: "equal" },
    ] } }));
    assert.deepEqual(rendered().map((item: { id: string }) => item.id), [row.id, "different-1", "different-2"]);
    await act(async () => root.unmount());
    clearTranscriptWindows();
    start = 1;
    end = 201;
    snapshot = Array.from({ length: 200 }, (_, index) => ({
      id: `tail-${index}`, type: "assistant", content: `tail ${index}`, seq: index + 1,
    }));
    root = createRoot(host);
    await act(async () => root.render(createElement(Harness)));
    await act(async () => handler?.({ payload: { ...authority, tab_id: "rpc",
      messages: [{ ...row, id: "older-i", content: "already live", seq: 0 }] } }));
    assert.equal(rendered().length, 201);
    await act(async () => { await loadOlder?.(); });
    assert.equal(rendered().length, 201, "backfill of an already-live item keeps one row");
    assert.deepEqual(rendered()[0], { id: "older-i", content: "already live" }, "older page cannot overwrite live content");
    console.log("RPC snapshot, completion, growth, backfill and equal-text identities passed");
  } finally {
    await act(async () => root.unmount());
    clearTranscriptWindows();
    __setInvokeForTest(null);
    __setListenForTest(null);
    await browser.happyDOM.abort();
  }
}
main().catch((error: unknown) => { console.error(error); process.exitCode = 1; });
