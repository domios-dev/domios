import assert from "node:assert/strict";
import { test } from "vitest";

import { formatAttachmentMarker, projectTranscript, type ChatSend } from "@codeterm/chat-core";
import { clearTranscriptWindows, commitTranscriptBatch, readTranscriptWindow } from "./transcriptStore";

const authority = { sourceSessionId: "session-1", sessionBindingNonce: "nonce-1" };
const marker = formatAttachmentMarker("C:/opaque/frag.png", "image");
const body = "opaque-a\nopaque-b";
const sentAtMs = Date.parse("2026-10-23T10:00:00.000Z");

function committedRow(id: string) {
  return {
    id, type: "user" as const, sourceSessionId: "session-1", timestamp: "2026-10-23T10:00:00.400Z",
    content: `\n\n<pasted_content id="ab">\n${body}\n</pasted_content id="ab">\n${marker}`,
    segments: [
      { type: "text" as const, text: "\n\n" },
      { type: "paste" as const, id: "ab", body: `\n${body}\n` },
      { type: "text" as const, text: "\n" },
      { type: "attachment" as const, kind: "image", path: "C:/opaque/frag.png", label: null, body: null, status: null },
    ],
  };
}

function project(rowIds: string[], sendIds: string[]) {
  clearTranscriptWindows();
  const key = "tab-1:session-1";
  commitTranscriptBatch(key, { kind: "delta", authority, range: null, rows: rowIds.map(committedRow) });
  return projectTranscript(readTranscriptWindow(key).transcript, sendIds.map((id) => ({
    id, correlationId: `${id}-token`, content: `${body}\n${marker}`, sentAtMs,
    echoBoundary: { ...authority, lastRowId: null },
  })));
}

test("a pasted send with an attachment renders one user bubble once its echo commits", () => {
  const projected = project(["committed-1"], ["send-1"]);
  assert.equal(projected.pending.length, 0);
  assert.deepEqual(projected.rows.filter((row) => row.type === "user").map((row) => row.id), ["committed-1"]);
});

test("two identical pasted sends keep two user bubbles", () => {
  const projected = project(["committed-1", "committed-2"], ["send-1", "send-2"]);
  assert.equal(projected.pending.length, 0);
  assert.equal(projected.rows.filter((row) => row.type === "user").length, 2);
});

const inlineBody = `opaque-a ${marker} \n\nopaque-b\n\nopaque-c`;

function inlineEcho(id: string, newline = "\n") {
  return {
    id, type: "user" as const, sourceSessionId: authority.sourceSessionId,
    timestamp: "2026-10-23T10:00:00.400Z",
    content: `\n\n<pasted_content id="ab">\n${inlineBody}\npasted by Domios\n</pasted_content id="ab">\n`
      .replaceAll("\n", newline),
    segments: [
      { type: "paste" as const, id: "ab", body: "\nopaque-a " },
      { type: "attachment" as const, kind: "image", path: "C:/opaque/frag.png", label: null, body: null, status: null },
      { type: "paste" as const, id: "ab", body: " \n\nopaque-b\n\nopaque-c" },
    ],
  };
}

function inlineSend(id: string): ChatSend {
  return {
    id, correlationId: `${id}-token`, content: inlineBody, sentAtMs,
    echoBoundary: { ...authority, lastRowId: "before-send" },
  };
}

function startInlineWindow() {
  clearTranscriptWindows();
  const key = "tab-inline:session-1";
  commitTranscriptBatch(key, { kind: "snapshot", authority, range: null, rows: [{
    id: "before-send", type: "assistant", content: "opaque-z", sourceSessionId: authority.sourceSessionId,
  }] });
  return key;
}

for (const newline of ["\n", "\r\n"]) {
  test(`a pasted attachment with an internal footer replaces its optimistic bubble (${JSON.stringify(newline)})`, () => {
    const key = startInlineWindow();
    const sends = [inlineSend("send-inline")];
    const before = projectTranscript(readTranscriptWindow(key).transcript, sends);
    assert.deepEqual(before.rows.filter((row) => row.type === "user").map((row) => row.id), ["send-inline"]);

    const row = inlineEcho("echo-inline", newline);
    commitTranscriptBatch(key, { kind: "delta", authority, range: null, rows: [row] });
    const projected = projectTranscript(readTranscriptWindow(key).transcript, sends);
    assert.equal(projected.pending.length, 0);
    assert.deepEqual(projected.rows.filter((item) => item.type === "user").map((item) => item.id), [row.id]);

    const recorded = sends.map((send) => ({ ...send, echo: projected.echoes.get(send.id) }));
    commitTranscriptBatch(key, { kind: "snapshot", authority, range: null, rows: [row] });
    const reattached = projectTranscript(readTranscriptWindow(key).transcript, recorded);
    assert.equal(reattached.pending.length, 0);
    assert.equal(reattached.rows.filter((item) => item.type === "user").length, 1);
  });
}

test("an internal paste footer consumes one optimistic send per attachment echo", () => {
  const key = startInlineWindow();
  const sends = [inlineSend("send-first"), inlineSend("send-second")];
  commitTranscriptBatch(key, { kind: "delta", authority, range: null, rows: [inlineEcho("echo-first")] });
  const first = projectTranscript(readTranscriptWindow(key).transcript, sends);
  assert.deepEqual(first.pending.map((send) => send.id), ["send-second"]);
  assert.equal(first.rows.filter((row) => row.type === "user").length, 2);

  commitTranscriptBatch(key, { kind: "delta", authority, range: null, rows: [inlineEcho("echo-second")] });
  const second = projectTranscript(readTranscriptWindow(key).transcript, sends);
  assert.equal(second.pending.length, 0);
  assert.equal(second.rows.filter((row) => row.type === "user").length, 2);
});
