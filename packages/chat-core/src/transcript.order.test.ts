import assert from "node:assert/strict";
import {
  createTranscriptState, projectTranscript, reduceTranscript, transcriptRows,
  type TranscriptBatch, type TranscriptState,
} from "./transcript";
import type { NormalizedChatMessage } from "./types";

const authority = { sourceSessionId: "ordered-session", sessionBindingNonce: "binding" };
function rows(start: number, count: number): NormalizedChatMessage[] {
  return Array.from({ length: count }, (_, index) => {
    const ordinal = start + index;
    return {
      id: `source-${ordinal}`,
      type: ordinal % 4 === 0 ? "user" : "assistant",
      content: String(ordinal),
      seq: index,
      timestamp: ordinal % 3 === 0 ? undefined : String(1_700_000_000_000 + ((ordinal * 7919) % 4473)),
      correlationId: ordinal === 4472 ? "send-token" : undefined,
    };
  });
}
function batch(kind: TranscriptBatch["kind"], start: number, end: number): TranscriptBatch {
  return {
    kind, authority,
    range: { start: start * 6500, end: end * 6500 },
    rows: rows(start, end - start),
  };
}
function assertOrder(state: TranscriptState, sends: Parameters<typeof projectTranscript>[1] = []) {
  const committed = transcriptRows(state);
  const ids = new Set(committed.map((row) => row.id));
  assert.deepEqual(
    projectTranscript(state, sends).rows.filter((row) => ids.has(row.id)).map((row) => row.content),
    committed.map((row) => row.content),
  );
}

let state = reduceTranscript(createTranscriptState(), batch("snapshot", 4000, 4470), 5000).state;
assertOrder(state);
state = reduceTranscript(state, batch("delta", 4470, 4471), 5000).state;
assertOrder(state);
state = reduceTranscript(state, batch("backfill", 2000, 4000), 5000).state;
assertOrder(state);
state = reduceTranscript(state, batch("delta", 4471, 4473), 5000).state;
state = reduceTranscript(state, batch("backfill", 0, 2000), 5000).state;
const sends = [{ id: "pending", content: "4472", sentAtMs: 1_699_999_000_000, correlationId: "send-token" }];
assertOrder(state, sends);
assert.equal(projectTranscript(state, sends).pending.length, 0);
assert.equal(projectTranscript(state, sends).rows.length, 4473);
const reconnect = reduceTranscript(state, batch("snapshot", 4000, 4473), 5000).state;
assertOrder(reconnect, sends);
const reload = reduceTranscript(createTranscriptState(), batch("snapshot", 0, 4473), 5000).state;
assertOrder(reload, sends);
assert.deepEqual(projectTranscript(reload, sends).rows.map((row) => row.content), rows(0, 4473).map((row) => row.content));

const unranged = reduceTranscript(createTranscriptState(), {
  kind: "snapshot", authority, range: null,
  rows: [
    { id: "late", type: "assistant", content: "2", seq: 2, timestamp: "1" },
    { id: "early", type: "assistant", content: "1", seq: 1, timestamp: "2" },
  ],
}).state;
assert.deepEqual(projectTranscript(unranged).rows.map((row) => row.content), ["1", "2"]);
const streamed = reduceTranscript(unranged, {
  kind: "delta", authority, range: null,
  rows: [
    { id: "late", type: "assistant", content: "updated", seq: 2, timestamp: "0" },
    { id: "next", type: "assistant", content: "3", seq: 3, timestamp: "-1" },
  ],
}).state;
assert.deepEqual(projectTranscript(streamed).rows.map((row) => row.content), ["1", "updated", "3"]);
console.log("PASS transcript sequence: 4473 rows, interleaved streaming/backfill, echo, reconnect, reload, unranged updates");
