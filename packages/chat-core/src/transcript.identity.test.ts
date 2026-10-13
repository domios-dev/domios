import assert from "node:assert/strict";
import {
  createTranscriptState, reduceTranscript, transcriptBackfillCursor,
  transcriptCommittedEnd, transcriptRows,
} from "./transcript";

const authority = { sourceSessionId: "own-session", sessionBindingNonce: "nonce" };
const item = { id: "provider-item", type: "assistant" as const, content: "prefix", seq: 1 };
let state = reduceTranscript(createTranscriptState(), {
  kind: "snapshot", authority, range: { unit: "items", start: 200, end: 201 }, rows: [item],
}).state;
assert.equal(transcriptCommittedEnd(state), null, "item paging has no byte cursor");
assert.equal(transcriptBackfillCursor(state), 200);
state = reduceTranscript(state, {
  kind: "delta", authority, range: null, rows: [{ ...item, content: "prefix complete" }],
}).state;
state = reduceTranscript(state, {
  kind: "backfill", authority, range: { unit: "items", start: 0, end: 200 }, rows: [item],
}).state;
assert.equal(transcriptBackfillCursor(state), 0);
assert.deepEqual(transcriptRows(state), [{ ...item, content: "prefix complete", sourceSessionId: "own-session" }]);
state = reduceTranscript(state, {
  kind: "delta", authority, range: null, rows: [
    { ...item, id: "other-1", content: "equal", seq: 2 },
    { ...item, id: "other-2", content: "equal", seq: 3 },
    { ...item, id: "other-3", content: "equal", seq: 4 },
    { ...item, id: "other-4", content: "equal", seq: 5 },
  ],
}).state;
assert.deepEqual(transcriptRows(state).map((row) => row.id), ["provider-item", "other-1", "other-2", "other-3", "other-4"]);

const thought = { id: "reasoning-rs-split-0", type: "thinking" as const, content: "thought" };
let bytes = reduceTranscript(createTranscriptState(), {
  kind: "snapshot", authority, range: { start: 0, end: 100 }, rows: [thought],
}).state;
bytes = reduceTranscript(bytes, {
  kind: "delta", authority, range: { start: 100, end: 200 }, rows: [thought],
}).state;
assert.deepEqual(transcriptRows(bytes).map((row) => row.id), [thought.id]);
assert.equal(transcriptCommittedEnd(bytes), 200);
console.log("PASS item paging, growing IDs, stale backfill, equal-text distinct IDs, split reasoning UUID");
