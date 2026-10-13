import { createTranscriptState, reduceTranscript, transcriptRows } from "./transcript";
import assert from "node:assert/strict";
const SESSION = { sourceSessionId: "sid-1", sessionBindingNonce: "nonce-1" };
function eq(actual: unknown, expected: unknown, name: string) {
  assert.deepEqual(actual, expected, name);
  console.log(`  ✓ ${name}`);
}

{
  const rows = (suffix: string) => [
    { id: `a${suffix}`, type: "assistant" as const, content: "A" },
    { id: `b${suffix}`, type: "assistant" as const, content: "B" },
  ];
  const snapshot = reduceTranscript(createTranscriptState(), {
    kind: "snapshot", authority: SESSION, range: null, rows: rows("1"),
  });
  const delta = reduceTranscript(snapshot.state, {
    kind: "delta", authority: SESSION, range: null, rows: rows("2"),
  });
  eq(transcriptRows(delta.state).map((row) => row.id), ["a1", "b1", "a2", "b2"], "unranged equal-text assistant batches retain every authoritative id");
  const replay = reduceTranscript(delta.state, {
    kind: "delta", authority: SESSION, range: null, rows: rows("2"),
  });
  eq(transcriptRows(replay.state).map((row) => row.id), ["a1", "b1", "a2", "b2"], "unranged replay of the same ids adds no rows");
}

console.log("2 passed, 0 failed");
