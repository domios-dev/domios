import assert from "node:assert/strict";
import { test } from "node:test";
import { mergeById } from "./merge";
import { createTranscriptState, reduceTranscript, transcriptRows } from "./transcript";
import type { NormalizedChatMessage } from "./types";

const authority = { sourceSessionId: "01a11ac0-thread", sessionBindingNonce: "binding" };

function turn(name: string, timestamp: string, seq: number): NormalizedChatMessage[] {
  return [
    { id: `${name}-user`, type: "user", content: name, timestamp, seq },
    { id: `${name}-answer`, type: "assistant", content: `${name} reply`, timestamp, seq: seq + 1 },
  ];
}

test("a resumed runtime thread keeps turns chronological when older history pages land after ready", () => {
  const early = turn("early", "2026-10-09T08:39:24.200Z", 0);
  const pong = turn("pong", "2026-10-15T12:34:54.971Z", 2);
  const latest = turn("latest", "2026-10-16T14:04:04.360Z", 4);
  let state = reduceTranscript(createTranscriptState(), {
    kind: "snapshot", authority, range: { start: 0, end: 1_025_219 }, rows: [...early, ...pong, ...latest],
  }).state;
  // Ready emits the cache after the newest history page only.
  state = reduceTranscript(state, { kind: "delta", authority, range: null, rows: turn("latest", latest[0].timestamp!, 46) }).state;
  // The next page prepends the older turn; a recovery attach re-emits the whole cache tail.
  state = reduceTranscript(state, {
    kind: "delta", authority, range: null, rows: [...turn("pong", pong[0].timestamp!, 48), ...turn("latest", latest[0].timestamp!, 50)],
  }).state;
  assert.deepEqual(transcriptRows(state).map(row => row.id), [
    "early-user", "early-answer", "pong-user", "pong-answer", "latest-user", "latest-answer",
  ]);
});

test("new incoming rows keep their source position before rows already merged", () => {
  const merged = mergeById(
    [{ id: "b", type: "user", content: "b", seq: 0 }],
    [{ id: "a", type: "user", content: "a", seq: 0 }, { id: "b", type: "user", content: "b2", seq: 1 }, { id: "c", type: "user", content: "c", seq: 2 }],
  );
  assert.deepEqual(merged.map(row => `${row.id}:${row.content}`), ["a:a", "b:b2", "c:c"]);
});
