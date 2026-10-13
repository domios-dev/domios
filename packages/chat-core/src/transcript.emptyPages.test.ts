import assert from "node:assert/strict";
import { test } from "node:test";
import {
  createTranscriptState, reduceTranscript, transcriptBackfillCursor, transcriptCommittedEnd, transcriptRows,
  type TranscriptBatch,
} from "./transcript";

const authority = { sourceSessionId: "session", sessionBindingNonce: "nonce" };
const batch = (kind: TranscriptBatch["kind"], start: number, end: number, content?: string): TranscriptBatch => ({
  kind, authority, range: { start, end }, rows: content ? [{ id: content, type: "assistant", content }] : [],
});

test("empty source pages consume ranges while zero-progress and foreign overlaps are refused", () => {
  const emptyInitial = reduceTranscript(createTranscriptState(), batch("snapshot", 200, 300));
  assert.equal(emptyInitial.outcome, "adopted");
  assert.equal(transcriptBackfillCursor(emptyInitial.state), 200);
  assert.equal(transcriptCommittedEnd(emptyInitial.state), 300);
  const initial = reduceTranscript(createTranscriptState(), batch("snapshot", 200, 300, "tail")).state;
  const emptyRecovery = reduceTranscript(initial, batch("snapshot", 200, 300));
  assert.deepEqual(transcriptRows(emptyRecovery.state), transcriptRows(initial));
  const emptyPage = reduceTranscript(initial, batch("backfill", 100, 200));
  assert.equal(emptyPage.outcome, "adopted");
  assert.equal(transcriptBackfillCursor(emptyPage.state), 100);
  assert.equal(transcriptCommittedEnd(emptyPage.state), 300);
  const older = reduceTranscript(emptyPage.state, batch("backfill", 0, 100, "older"));
  assert.deepEqual(transcriptRows(older.state).map((row) => row.content), ["older", "tail"]);
  assert.equal(transcriptBackfillCursor(older.state), 0);
  assert.equal(reduceTranscript(older.state, batch("backfill", 0, 0)).outcome, "ignored_empty");
  assert.equal(reduceTranscript(initial, batch("backfill", 100, 250)).outcome, "rejected_range");
  const delta = reduceTranscript(initial, batch("delta", 300, 400));
  assert.equal(delta.outcome, "adopted");
  assert.equal(transcriptCommittedEnd(delta.state), 400);
});

test("bounded snapshot preserves backfilled ranges beyond the tail retention limit", () => {
  let state = reduceTranscript(createTranscriptState(), batch("snapshot", 600, 800, "tail")).state;
  state = reduceTranscript(state, {
    ...batch("backfill", 0, 600),
    rows: Array.from({ length: 600 }, (_, index) => ({ id: String(index), type: "assistant", content: `older-${index}` })),
  }).state;
  const refreshed = reduceTranscript(state, batch("snapshot", 600, 800, "refreshed"));
  assert.equal(transcriptRows(refreshed.state).length, 601);
  assert.equal(transcriptBackfillCursor(refreshed.state), 0);
  assert.equal(transcriptRows(refreshed.state).at(-1)?.content, "refreshed");
});
