import assert from "node:assert/strict";
import {
  createTranscriptState,
  reduceTranscript,
  transcriptRows,
  type TranscriptBatch,
} from "./transcript";

const authority = {
  sourceSessionId: "session",
  sessionBindingNonce: "binding",
};
const rows = (start: number, count: number): TranscriptBatch["rows"] =>
  Array.from({ length: count }, (_, index) => ({
    id: `row-${start + index}`,
    type: index % 2 ? "tool_result" : "assistant",
    content: `payload-${start + index}`,
    seq: start + index,
  }));

for (const kind of ["snapshot", "delta"] as const) {
  let state = createTranscriptState();
  for (let index = 0; index < 1_000; index += 10) {
    state = reduceTranscript(
      state,
      {
        kind,
        authority,
        range: null,
        rows: rows(index, 10),
      },
      25,
    ).state;
    assert(transcriptRows(state).length <= 25);
  }
  assert.equal(state.evictedOlder, 975);
  assert.deepEqual(
    transcriptRows(state).map((row) => row.id),
    rows(975, 25).map((row) => row.id),
  );
  const updated = reduceTranscript(
    state,
    {
      kind: "delta",
      authority,
      range: null,
      rows: [
        {
          id: "row-999",
          type: "assistant",
          content: "stream finished",
          seq: 999,
        },
      ],
    },
    25,
  ).state;
  assert.equal(transcriptRows(updated).length, 25);
  assert.equal(transcriptRows(updated).at(-1)?.content, "stream finished");
  assert.equal(updated.evictedOlder, 975);
  assert.equal(transcriptRows(state).at(-1)?.content, "payload-999");
}

const oversized = reduceTranscript(
  createTranscriptState(),
  {
    kind: "snapshot",
    authority,
    range: null,
    rows: rows(0, 100),
  },
  25,
).state;
assert.equal(transcriptRows(oversized).length, 25);
assert.equal(oversized.evictedOlder, 75);
const rotated = reduceTranscript(
  oversized,
  {
    kind: "delta",
    authority: { ...authority, sourceSessionId: "next" },
    range: null,
    rows: rows(100, 30),
  },
  25,
);
assert.equal(rotated.outcome, "rotated");
assert.equal(rotated.state.evictedOlder, 5);
assert.equal(transcriptRows(rotated.state)[0].id, "row-105");

console.log(
  "Unranged transcript retention, streaming updates, and rotation passed",
);
