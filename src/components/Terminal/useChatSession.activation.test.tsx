import assert from "node:assert/strict";
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import type { NormalizedChatMessage } from "@codeterm/chat-core";

const dom = new Window({ url: "http://localhost/" });
for (const [key, value] of Object.entries({ window: dom, document: dom.document,
  navigator: dom.navigator, HTMLElement: dom.HTMLElement, Event: dom.Event, CustomEvent: dom.CustomEvent,
  IS_REACT_ACT_ENVIRONMENT: true })) {
  Object.defineProperty(globalThis, key, { configurable: true, value });
}

const cursor = (offset: number) => JSON.stringify({ kind: "bytes", incarnation: "file", offset });
const authority = { source_session_id: "own-session", session_binding_nonce: "own-nonce" };
const row = (id: string): NormalizedChatMessage => ({ id, type: "assistant", content: id });

async function main() {
  const bridge = await import("../../lib/bridge");
  const { useChatSession } = await import("./useChatSession");
  const { clearTranscriptWindows } = await import("./transcriptStore");
  const handlers = new Map<string, (event: { payload: unknown }) => void>();
  bridge.__setListenForTest((async (event: string, handler: (event: { payload: unknown }) => void) => {
    handlers.set(event, handler);
    return () => { handlers.delete(event); };
  }) as never);
  let rows = [row("initial")];
  let offset = 100;
  let starts = 0;
  let stops = 0;
  const reads: string[] = [];
  bridge.__setInvokeForTest((async (command: string, args: Record<string, unknown>) => {
    if (command === "chat_start") {
      starts++;
      if (starts > 1) assert.equal(args.knownWindow, undefined, "gap attachment must request an authoritative snapshot");
      return { ...authority, messages: [...rows], start_offset: 0, offset, cursor: cursor(offset),
        subscription_id: args.subscriptionId, pending: false };
    }
    if (command === "chat_stop") stops++;
    if (command === "chat_resume") {
      reads.push(String(args.cursor));
      const from = JSON.parse(String(args.cursor)).offset as number;
      return { kind: "resumed", caught_up: true, deltas: from === offset ? [] : [{ ...authority,
        messages: rows.slice((from - 100) / 10 + 1), start_offset: from, offset, cursor: cursor(offset) }] };
    }
    return {};
  }) as never);
  let shown: string[] = [];
  function Harness({ active }: { active: boolean }) {
    const session = useChatSession({ tabId: "activation", agentSessionId: "own-session",
      sessionBindingNonce: "own-nonce", active, retainAttachment: true, live: active });
    shown = session.allMessages.map(message => message.id);
    return null;
  }
  const root = createRoot(document.createElement("div"));
  const render = async (active: boolean) => { await act(async () => root.render(createElement(Harness, { active }))); };
  const append = (id: string, deliver: boolean) => {
    const from = offset;
    rows = [...rows, row(id)];
    offset += 10;
    if (deliver) handlers.get("tab-chat-msg-activation")?.({ payload: { ...authority,
      tab_id: "activation", messages: [row(id)], start_offset: from, offset, cursor: cursor(offset) } });
  };
  try {
    await render(true);
    assert.deepEqual(shown, ["initial"]);
    assert.equal(reads.length, 0, "initial attachment already reconciles");
    await render(false);
    await act(async () => { append("hidden-delivered", true); append("hidden-lost", false); });
    assert.deepEqual(shown, ["initial"], "hidden rendering stays held");
    await render(true);
    assert.deepEqual(reads, [cursor(110)], "activation reads once from the last adopted cursor");
    assert.deepEqual(shown, ["initial", "hidden-delivered", "hidden-lost"]);
    assert.equal(starts, 1, "activation retains the existing stream lease");
    assert.equal(stops, 0);
    await render(true);
    assert.equal(reads.length, 1, "ordinary render is not activation");
    console.log("PASS hidden final event reconciles on activation from the committed cursor");

    await act(async () => { append("gap-lost", false); append("after-gap", true); });
    assert.deepEqual(reads, [cursor(110)], "a proven gap uses a snapshot instead of cursor replay");
    assert.deepEqual(shown, ["initial", "hidden-delivered", "hidden-lost", "gap-lost", "after-gap"]);
    assert.equal(starts, 2);
    console.log("PASS sequence gap repairs without remounting or advancing past missing bytes");

    Object.defineProperty(document, "visibilityState", { configurable: true, value: "hidden" });
    await act(async () => document.dispatchEvent(new Event("visibilitychange")));
    await act(async () => append("background-lost", false));
    Object.defineProperty(document, "visibilityState", { configurable: true, value: "visible" });
    await act(async () => document.dispatchEvent(new Event("visibilitychange")));
    assert.deepEqual(reads, [cursor(110), cursor(140)]);
    assert.equal(shown[shown.length - 1], "background-lost");
    assert.equal(starts, 2);
    console.log("PASS window visibility return reconciles the selected tab");
  } finally {
    await act(async () => root.unmount());
    bridge.__setInvokeForTest(null);
    bridge.__setListenForTest(null);
    clearTranscriptWindows();
    await dom.happyDOM.abort();
    dom.close();
  }
}

main().then(() => process.exit(0)).catch(error => { console.error(error); process.exit(1); });
