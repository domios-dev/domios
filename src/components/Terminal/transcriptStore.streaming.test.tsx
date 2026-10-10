import assert from "node:assert/strict";
import { act } from "react";
import test from "node:test";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { transcriptRows } from "@codeterm/chat-core";
import { clearTranscriptWindows, commitTranscriptBatch, readTranscriptWindow, resetTranscriptWindow, useTranscriptWindow } from "./transcriptStore";

test("stream updates preserve ordered text and coalesce paint", async () => {
  const window = new Window();
  for (const [key, value] of Object.entries({ window, document: window.document, navigator: window.navigator, IS_REACT_ACT_ENVIRONMENT: true })) {
    Object.defineProperty(globalThis, key, { configurable: true, value });
  }
  const callbacks = new Map<number, FrameRequestCallback>();
  let next = 0;
  globalThis.requestAnimationFrame = (callback) => { callbacks.set(++next, callback); return next; };
  globalThis.cancelAnimationFrame = (handle) => { callbacks.delete(handle); };
  const key = "streaming-regression:session";
  const authority = { sourceSessionId: "session", sessionBindingNonce: "nonce" };
  const host = document.createElement("div");
  const root = createRoot(host);
  let renders = 0;
  function View() {
    renders++;
    const { transcript } = useTranscriptWindow(key);
    return <>{transcriptRows(transcript).map((row) => row.content).join("")}</>;
  }
  await act(async () => root.render(<View />));
  const initialRenders = renders;
  let source = "";
  for (let line = 0; line < 600; line++) {
    source += `${line + 1}. Trees 🌲 grow.\n`;
    assert.equal(commitTranscriptBatch(key, { kind: "delta", authority, range: null,
      rows: [{ id: "answer", type: "assistant", content: source, seq: 0 }],
    }), "adopted");
    assert.equal(transcriptRows(readTranscriptWindow(key).transcript)[0].content, source);
  }
  assert.equal(renders, initialRenders);
  assert.equal(callbacks.size, 1);
  await act(async () => {
    const pending = [...callbacks.values()];
    callbacks.clear();
    pending.forEach((callback) => callback(0));
  });
  assert.equal(renders, initialRenders + 1);
  assert.equal(host.textContent, source);
  commitTranscriptBatch(key, { kind: "delta", authority, range: null,
    rows: [{ id: "answer", type: "assistant", content: source + "pending", seq: 0 }],
  });
  await act(async () => resetTranscriptWindow(key));
  assert.equal(callbacks.size, 0);
  assert.equal(host.textContent, "");
  await act(async () => root.unmount());
  clearTranscriptWindows();
  console.log("600 ordered updates remain synchronously readable and paint once; reset cancels stale paint");
});
