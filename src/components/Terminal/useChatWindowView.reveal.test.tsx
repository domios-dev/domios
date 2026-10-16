import assert from "node:assert/strict";
import { test } from "node:test";
import { act, createElement, useRef, useState } from "react";
import { flushSync } from "react-dom";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { createTranscriptState, reduceTranscript, type TranscriptState } from "@codeterm/chat-core";
import { useChatWindowView } from "./useChatWindowView";
import { clearTranscriptWindows, useTranscriptWindow } from "./transcriptStore";

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
  const initial = page(createTranscriptState(), 220, 440);
  function Harness() {
    const [transcript, setTranscript] = useState(initial);
    const followTailRef = useRef(true);
    const { anchorKey } = useTranscriptWindow("reveal:session");
    view = useChatWindowView({ tabId: "reveal", windowKey: "reveal:session", transcript, anchorKey, followTailRef,
      loadOlderPage: async (cursor) => {
        requests++;
        assert.equal(cursor, 220);
        flushSync(() => setTranscript(page(initial, 0, 220)));
        return true;
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
    assert.equal(view?.allMessages.length, 440);
    assert.equal(host.textContent, "row-60");
  } finally {
    await act(async () => root.unmount());
    clearTranscriptWindows();
    await browser.happyDOM.abort();
  }
});
