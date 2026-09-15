import assert from "node:assert/strict";
import test from "node:test";

import { projectTranscript, transcriptRows, type TranscriptBatch } from "@codeterm/chat-core";
import type { ChatSendStatus } from "./chatSendStatus";
import {
  clearTranscriptWindows,
  commitTranscriptBatch,
  readTranscriptWindow,
  stampConfirmedTranscriptRow,
  trackTranscriptSend,
} from "./transcriptStore";

const authority = { sourceSessionId: "session-1", sessionBindingNonce: "nonce-1" };
const SENT_AT = 1_700_000_000_000;

function status(sendId: string, correlationId: string, state: ChatSendStatus["state"]): ChatSendStatus {
  return {
    send_id: sendId,
    tab_id: "tab-1",
    state,
    error: null,
    ledger_id: "ledger-1",
    sequence: 1,
    retained_from: 1,
    correlationId,
  };
}

function commitUsers(key: string, rows: TranscriptBatch["rows"]): void {
  commitTranscriptBatch(key, {
    kind: "delta",
    authority,
    range: null,
    rows,
  });
}

test("confirmed status stamps the committed user row and retires a chip-divergent send by id", () => {
  clearTranscriptWindows();
  const key = "tab-1:session-1";
  const token = "optimistic-token";
  trackTranscriptSend(key, status("send-1", token, "accepted"), SENT_AT);
  commitUsers(key, [{
    id: "committed-1",
    type: "user",
    content: "◆ task_x — enriched chip label ◆",
    timestamp: String(SENT_AT + 1),
  }]);

  assert(stampConfirmedTranscriptRow(key, status("send-1", token, "confirmed")));
  const transcript = readTranscriptWindow(key).transcript;
  assert.equal(transcriptRows(transcript)[0]?.correlationId, token);

  const projected = projectTranscript(transcript, [{
    id: token,
    content: "see task_x",
    sentAtMs: SENT_AT,
    correlationId: token,
  }]);
  assert.equal(projected.pending.length, 0);
  assert.equal(projected.rows.filter((row) => row.type === "user").length, 1);
  assert.equal(projected.rows[0]?.id, "committed-1");
});

test("confirmed send ids stamp distinct same-text turns with their own tokens", () => {
  clearTranscriptWindows();
  const key = "tab-1:session-1";
  trackTranscriptSend(key, status("send-a", "token-a", "accepted"), SENT_AT);
  trackTranscriptSend(key, status("send-b", "token-b", "accepted"), SENT_AT);
  commitUsers(key, [
    { id: "committed-a", type: "user", content: "same text", timestamp: String(SENT_AT + 100) },
    { id: "committed-b", type: "user", content: "same text", timestamp: String(SENT_AT + 200) },
  ]);

  assert(stampConfirmedTranscriptRow(key, status("send-a", "token-a", "confirmed")));
  assert(stampConfirmedTranscriptRow(key, status("send-b", "token-b", "confirmed")));
  const rows = transcriptRows(readTranscriptWindow(key).transcript);
  assert.deepEqual(rows.map((row) => row.correlationId), ["token-a", "token-b"]);
  assert.equal(rows.filter((row) => row.type === "user").length, 2);
});

test("a user row stamped before the send, or with no time at all, is never stamped as its echo", () => {
  clearTranscriptWindows();
  const key = "tab-1:session-1";
  commitUsers(key, [
    { id: "history", type: "user", content: "same text", timestamp: String(SENT_AT - 60_000) },
    { id: "untimed", type: "user", content: "same text" },
  ]);
  trackTranscriptSend(key, status("send-late", "token-late", "accepted"), SENT_AT);
  assert.equal(stampConfirmedTranscriptRow(key, status("send-late", "token-late", "confirmed")), false);
  assert.deepEqual(transcriptRows(readTranscriptWindow(key).transcript).map((row) => row.correlationId), [undefined, undefined]);
});
