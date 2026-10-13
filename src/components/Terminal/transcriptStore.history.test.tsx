import assert from "node:assert/strict";
import { test } from "node:test";
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { transcriptRows, type TranscriptBatch } from "@codeterm/chat-core";
import {
  clearTranscriptWindows, commitTranscriptBatch, readTranscriptWindow,
  reconcileTranscriptWindow, resetTranscriptWindow, setTranscriptAnchor, useTranscriptWindow,
} from "./transcriptStore";

const authority = { sourceSessionId: "session", sessionBindingNonce: "nonce" };
const batch = (start: number, end: number, kind: TranscriptBatch["kind"] = "snapshot"): TranscriptBatch => ({
  kind, authority, range: { start, end },
  rows: Array.from({ length: end - start }, (_, index) => ({
    id: `item-${start + index}`, type: "assistant", content: `answer-${start + index}`,
  })),
});

test("bounded same-authority snapshot preserves loaded history and detached row", () => {
  clearTranscriptWindows();
  commitTranscriptBatch("A", batch(200, 400));
  commitTranscriptBatch("A", batch(0, 200, "backfill"));
  setTranscriptAnchor("A", "0:50");
  reconcileTranscriptWindow("A", batch(200, 400));
  const window = readTranscriptWindow("A");
  assert.equal(transcriptRows(window.transcript).length, 400);
  assert.equal(window.anchorKey, "0:50");
  assert.equal(transcriptRows(window.transcript).find((row) => row.id === window.anchorKey)?.content, "answer-50");
  resetTranscriptWindow("A");
  reconcileTranscriptWindow("A", batch(200, 400));
  assert.equal(transcriptRows(readTranscriptWindow("A").transcript).length, 200);
  assert.equal(readTranscriptWindow("A").anchorKey, null);
  clearTranscriptWindows();
});

test("cache pressure protects every subscribed window and reclaims released windows", async () => {
  const browser = new Window();
  Object.defineProperties(globalThis, {
    window: { configurable: true, value: browser },
    document: { configurable: true, value: browser.document },
    IS_REACT_ACT_ENVIRONMENT: { configurable: true, value: true },
  });
  clearTranscriptWindows();
  const host = document.createElement("main");
  const root = createRoot(host);
  function Readers() {
    return createElement("div", null, ...Array.from({ length: 9 }, (_, index) =>
      createElement(Reader, { key: index, name: `reader-${index}` })));
  }
  function Reader({ name }: { name: string }) {
    const window = useTranscriptWindow(name);
    return createElement("output", null, transcriptRows(window.transcript).map((row) => row.content).join(","));
  }
  try {
    await act(async () => root.render(createElement(Readers)));
    await act(async () => {
      for (let index = 0; index < 9; index++) commitTranscriptBatch(`reader-${index}`, batch(0, 1));
      commitTranscriptBatch("reader-0", batch(1, 2, "delta"));
    });
    assert.equal(host.querySelector("output")?.textContent, "answer-0,answer-1");
    for (let index = 0; index < 9; index++) {
      assert.ok(transcriptRows(readTranscriptWindow(`reader-${index}`).transcript).some((row) => row.content === "answer-0"));
    }
    await act(async () => root.unmount());
    commitTranscriptBatch("unsubscribed", batch(0, 1));
    assert.equal(readTranscriptWindow("reader-1").transcript.authority, null);
  } finally {
    await act(async () => root.unmount());
    clearTranscriptWindows();
    await browser.happyDOM.abort();
  }
});
