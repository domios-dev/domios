import assert from "node:assert/strict";
import { afterEach, test } from "node:test";
import {
  clearTranscriptWindows,
  forgetTranscriptWindowCursor,
  markTranscriptSnapshotReady,
  readTranscriptMemo,
  readTranscriptWindow,
  retainedTranscriptMemo,
  rememberTranscriptWindow,
  resetTranscriptWindow,
} from "./transcriptStore";

afterEach(clearTranscriptWindows);

test("memo bound evicts the least recently used key", () => {
  const known = {
    source_session_id: "own", session_binding_nonce: null,
    cursor: "cursor", end_offset: 42, file_length: 42,
  };
  for (let index = 0; index < 256; index++) rememberTranscriptWindow(`memo-${index}`, known);
  assert.deepEqual(readTranscriptMemo("memo-0"), known);
  rememberTranscriptWindow("memo-new", known);
  assert.equal(readTranscriptMemo("memo-1"), undefined);
  assert.deepEqual(readTranscriptMemo("memo-0"), known);
  assert.deepEqual(readTranscriptMemo("memo-new"), known);
});

test("row eviction preserves the known window memo without claiming retained rows", () => {
  const known = {
    source_session_id: "own", session_binding_nonce: "nonce",
    cursor: "cursor", end_offset: 42, file_length: 42,
  };
  rememberTranscriptWindow("old", known);
  markTranscriptSnapshotReady("old", "nonce");
  assert.deepEqual(retainedTranscriptMemo("old"), known);
  for (let index = 0; index < 15; index++) markTranscriptSnapshotReady(`other-${index}`, "nonce");
  assert.equal(readTranscriptWindow("old").snapshotBinding, undefined);
  assert.equal(retainedTranscriptMemo("old"), undefined);
  assert.deepEqual(readTranscriptMemo("old"), known);
  forgetTranscriptWindowCursor("old");
  assert.equal(readTranscriptMemo("old"), undefined);
});

test("reset and global clear discard independently retained memos", () => {
  const known = {
    source_session_id: "own", session_binding_nonce: null,
    cursor: "cursor", end_offset: 42, file_length: 42,
  };
  rememberTranscriptWindow("reset", known);
  resetTranscriptWindow("reset");
  assert.equal(readTranscriptMemo("reset"), undefined);
  rememberTranscriptWindow("clear", known);
  clearTranscriptWindows();
  assert.equal(readTranscriptMemo("clear"), undefined);
});
