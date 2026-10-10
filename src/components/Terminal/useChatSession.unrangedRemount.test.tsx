import assert from "node:assert/strict";
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { TRANSCRIPT_RETENTION_LIMIT, type NormalizedChatMessage } from "@codeterm/chat-core";
import { __setInvokeForTest, __setListenForTest } from "../../lib/bridge";
import { clearTranscriptWindows } from "./transcriptStore";
import { useChatSession } from "./useChatSession";
import type { ChatMsgEvent } from "./chatSessionAuthority";

const browser = new Window();
Object.defineProperties(globalThis, {
  window: { configurable: true, value: browser },
  document: { configurable: true, value: browser.document },
  navigator: { configurable: true, value: browser.navigator },
  IS_REACT_ACT_ENVIRONMENT: { configurable: true, value: true },
});

async function scenario(source: string, unbound = false) {
  clearTranscriptWindows();
  const session = unbound ? null : source;
  let handler: ((event: { payload: ChatMsgEvent }) => void) | undefined;
  let snapshot: NormalizedChatMessage[] = [];
  let pending = false;
  let nonce = "nonce";
  let starts = 0;
  let stops = 0;
  __setListenForTest(async (_name, callback) => {
    handler = callback as typeof handler;
    return () => { handler = undefined; };
  });
  const invoke = async (command: string, args?: Record<string, unknown>): Promise<unknown> => {
    if (command === "chat_send_status") return [];
    if (command === "chat_stop") { stops++; return; }
    if (command === "chat_start") {
      starts++;
      return { text: "", start_offset: 0, offset: 0, pending,
        messages: snapshot, source_session_id: session, session_binding_nonce: nonce,
        subscription_id: args?.subscriptionId };
    }
    return {};
  };
  __setInvokeForTest(invoke as Parameters<typeof __setInvokeForTest>[0]);
  const host = document.createElement("main");
  document.body.append(host);
  function Harness() {
    const chat = useChatSession({ tabId: source, agentSessionId: session,
      sessionBindingNonce: nonce, allowUnboundAuthority: unbound });
    return createElement("output", null, JSON.stringify(chat.allMessages.map((row) => row.content)));
  }
  const mount = async () => {
    const root = createRoot(host);
    await act(async () => { root.render(createElement(Harness)); });
    return root;
  };
  const rows = Array.from({ length: 500 }, (_, index): NormalizedChatMessage => ({
    id: `row-${index}`, type: "assistant", content: `row-${index}`, seq: index,
  }));
  const rendered = () => JSON.parse(host.textContent ?? "[]") as string[];
  const expected = rows.slice(-TRANSCRIPT_RETENTION_LIMIT).map((row) => row.content);
  let root = await mount();
  await act(async () => {
    for (const row of rows) handler?.({ payload: { tab_id: source, messages: [row],
      source_session_id: session, session_binding_nonce: nonce } });
  });
  assert.deepEqual(rendered(), expected);
  await act(async () => root.unmount());
  assert.equal(handler, undefined);
  root = await mount();
  assert.deepEqual(rendered(), expected, `${source}: empty store replay discarded streamed history`);
  await act(async () => root.unmount());
  clearTranscriptWindows();
  snapshot = rows;
  root = await mount();
  assert.deepEqual(rendered(), expected, `${source}: snapshot replay changed retained history`);
  await act(async () => root.unmount());
  pending = true;
  snapshot = [];
  root = await mount();
  await act(async () => handler?.({ payload: { tab_id: source,
    messages: [{ id: "new-epoch", type: "assistant", content: "new-epoch" }],
    source_session_id: session, session_binding_nonce: nonce } }));
  assert.deepEqual(rendered(), ["new-epoch"], `${source}: pending replay retained the prior epoch`);
  await act(async () => root.unmount());
  pending = false;
  nonce = "rotated";
  snapshot = [];
  root = await mount();
  assert.deepEqual(rendered(), [], `${source}: prior binding leaked into a new authority`);
  await act(async () => root.unmount());
  assert.equal(starts, 5);
  assert.equal(stops, 5);
  host.remove();
}

async function main() {
  try {
    await scenario("structured-sse");
    await scenario("structured-rpc");
    await scenario("chat-backend", true);
  } finally {
    clearTranscriptWindows();
    __setInvokeForTest(null);
    __setListenForTest(null);
    await browser.happyDOM.abort();
  }
  console.log("Unranged remount, replay, authority rotation and retention passed");
}
main().catch((error: unknown) => { console.error(error); process.exitCode = 1; });
