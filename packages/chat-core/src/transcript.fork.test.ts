import assert from "node:assert/strict";
import test from "node:test";
import { createTranscriptState, reduceTranscript, transcriptRows, projectTranscript } from "./transcript";

const authority = { sourceSessionId: "fork", sessionBindingNonce: "nonce" };
const inherited = { id: "source-row", type: "user" as const, content: "same", inheritedFromSessionId: "source" };

test("inherited history is replaced atomically by materialized fork rows", () => {
  const initial = reduceTranscript(createTranscriptState(), {
    kind: "snapshot", authority, range: { start: 0, end: 0 }, rows: [inherited],
  }).state;
  const empty = reduceTranscript(initial, {
    kind: "snapshot", authority, range: { start: 0, end: 0 }, rows: [],
  });
  assert.deepEqual(transcriptRows(empty.state), transcriptRows(initial));
  const rows = [{ id: "fork-row", type: "user" as const, content: "same" },
    { id: "new-row", type: "assistant" as const, content: "new" }];
  const live = { kind: "delta" as const, authority, range: { start: 0, end: 200 }, rows };
  const materialized = reduceTranscript(initial, live).state;
  assert.deepEqual(transcriptRows(materialized).map(row => row.id), ["fork-row", "new-row"]);
  assert.deepEqual(transcriptRows(reduceTranscript(materialized, live).state), transcriptRows(materialized));
  const late = reduceTranscript(materialized, {
    kind: "snapshot", authority, range: { start: 0, end: 0 }, rows: [inherited],
  });
  assert.equal(late.outcome, "ignored_stale");
  assert.deepEqual(transcriptRows(late.state), transcriptRows(materialized));
});

test("inherited user rows never reconcile a fork send", () => {
  const state = reduceTranscript(createTranscriptState(), {
    kind: "snapshot", authority, range: { start: 0, end: 0 },
    rows: [{ ...inherited, correlationId: "send-token", timestamp: "1000" }],
  }).state;
  const send = { id: "send", content: "same", sentAtMs: 1000, correlationId: "send-token" };
  const projected = projectTranscript(state, [send]);
  assert.equal(projected.echoes.size, 0);
  assert.deepEqual(projected.pending, [send]);
});
