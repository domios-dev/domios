import assert from "node:assert/strict";
import { test } from "vitest";

import { projectTranscript, transcriptRows, type TranscriptBatch } from "@codeterm/chat-core";
import type { ChatSendStatus } from "./chatSendStatus";
import {
  clearTranscriptWindows,
  commitTranscriptBatch,
  readTranscriptWindow,
  stampConfirmedTranscriptRow,
} from "./transcriptStore";

const authority = { sourceSessionId: "session-1", sessionBindingNonce: "nonce-1" };

function status(sendId: string, correlationId: string, itemId: string): ChatSendStatus {
  return {
    send_id: sendId,
    tab_id: "tab-1",
    state: "delivered",
    error: null,
    ledger_id: "ledger-1",
    sequence: 1,
    retained_from: 1,
    correlationId,
    matched_row: {
      source: { source_session_id: "session-1", record: {
        kind: "structured", thread_id: "session-1", item_id: itemId,
      } },
      ordinal: 1,
    },
  };
}

function commitUsers(key: string, rows: TranscriptBatch["rows"]): void {
  commitTranscriptBatch(key, { kind: "delta", authority, range: null, rows });
}

test("delivered status stamps only its exact structured row without using time or body", () => {
  clearTranscriptWindows();
  const key = "tab-1:session-1";
  commitUsers(key, [{ id: "committed-1", type: "user", content: "enriched provider row" }]);

  assert(stampConfirmedTranscriptRow(key, status("send-1", "token-1", "committed-1")));
  const transcript = readTranscriptWindow(key).transcript;
  assert.equal(transcriptRows(transcript)[0]?.correlationId, "token-1");

  const projected = projectTranscript(transcript, [{
    id: "optimistic-1",
    correlationId: "token-1",
    content: "composer payload",
    sentAtMs: 1_700_000_000_000,
    matchedRow: status("send-1", "token-1", "committed-1").matched_row,
    echoBoundary: { ...authority, lastRowId: null },
  }]);
  assert.equal(projected.pending.length, 0);
  assert.equal(projected.rows.filter((row) => row.type === "user").length, 1);
  assert.equal(projected.rows[0]?.id, "committed-1");
});

test("identical sends stamp their own exact structured rows", () => {
  clearTranscriptWindows();
  const key = "tab-1:session-1";
  commitUsers(key, [
    { id: "committed-a", type: "user", content: "same text" },
    { id: "committed-b", type: "user", content: "same text" },
  ]);

  assert(stampConfirmedTranscriptRow(key, status("send-a", "token-a", "committed-a")));
  assert(stampConfirmedTranscriptRow(key, status("send-b", "token-b", "committed-b")));
  const rows = transcriptRows(readTranscriptWindow(key).transcript);
  assert.deepEqual(rows.map((row) => row.correlationId), ["token-a", "token-b"]);
  assert.equal(rows.filter((row) => row.type === "user").length, 2);
});

test("foreign session evidence cannot stamp a row into the current transcript", () => {
  clearTranscriptWindows();
  const key = "tab-1:session-1";
  commitUsers(key, [{ id: "foreign-item", type: "user", content: "same text", sourceSessionId: "session-2" }]);
  const foreign = {
    ...status("send-foreign", "token-foreign", "foreign-item"),
    matched_row: {
      source: { source_session_id: "session-2", record: {
        kind: "structured" as const, thread_id: "session-2", item_id: "foreign-item",
      } },
      ordinal: 1,
    },
  };

  assert.equal(stampConfirmedTranscriptRow(key, foreign), false);
  assert.equal(transcriptRows(readTranscriptWindow(key).transcript)[0]?.correlationId, undefined);
});

test("JSONL matched-row identity is retained but not approximated as a structured row id", () => {
  clearTranscriptWindows();
  const key = "tab-1:session-1";
  commitUsers(key, [{ id: "rec-40-0", type: "user", content: "same text" }]);
  const jsonl: ChatSendStatus = {
    ...status("send-jsonl", "token-jsonl", "rec-40-0"),
    matched_row: {
      source: { source_session_id: "session-1", record: {
        kind: "jsonl", incarnation: "incarnation-1", record_start: 40, record_row: 0,
      } },
      ordinal: 1,
    },
  };

  assert.equal(stampConfirmedTranscriptRow(key, jsonl), false);
  assert.equal(transcriptRows(readTranscriptWindow(key).transcript)[0]?.correlationId, undefined);
});
