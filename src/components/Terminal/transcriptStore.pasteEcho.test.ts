import assert from "node:assert/strict";
import { test } from "vitest";

import { formatAttachmentMarker, projectTranscript } from "@codeterm/chat-core";
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
