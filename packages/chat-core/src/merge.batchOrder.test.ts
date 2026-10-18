import assert from "node:assert/strict";
import { test } from "node:test";
import { mergeById } from "./merge";
import { createTranscriptState, reduceTranscript, transcriptRows } from "./transcript";
import type { NormalizedChatMessage } from "./types";

function row(id: string, seq: number, content = id): NormalizedChatMessage {
  return { id, seq, content, type: "assistant" };
}

test("a delta's local ordinals cannot move new rows before committed history", () => {
  const history = [row("old-context", 0), row("old-user", 1), row("old-answer", 2)];
  assert.deepEqual(mergeById(history, [row("new", 0)]).map(message => message.id), [
    "old-context", "old-user", "old-answer", "new",
  ]);
  const added = mergeById(history, [row("reply", 1), row("prompt", 0)]);
  assert.deepEqual(added.map(message => message.id), ["old-context", "old-user", "old-answer", "prompt", "reply"]);
  const amended = mergeById(added, [row("reply", 0, "complete")]);
  assert.deepEqual(amended.map(message => message.id), added.map(message => message.id));
  assert.equal(amended[amended.length - 1]?.content, "complete");
  const repeated = mergeById(history, [row("reply", 10, "old"), row("reply", 0, "latest")]);
  assert.equal(repeated[repeated.length - 1]?.content, "latest");
});

test("an amendment keeps its committed slot despite a changed ordinal", () => {
  const history = [row("before", 0), row("amended", 1), row("after", 2)];
  const amended = mergeById(history, [row("amended", 20, "complete")]);
  assert.deepEqual(amended.map(message => message.id), history.map(message => message.id));
  assert.equal(amended[1]?.content, "complete");
});

test("unranged streaming batches with restarted ordinals advance the transcript tail", () => {
  const authority = { sourceSessionId: "session", sessionBindingNonce: "binding" };
  let state = reduceTranscript(createTranscriptState(), {
    kind: "snapshot", authority, range: null,
    rows: Array.from({ length: 230 }, (_, index) => row(`history-${index}`, index)),
  }).state;
  for (const message of [row("prompt", 0), row("answer", 0, "partial"), row("answer", 0, "complete")]) {
    state = reduceTranscript(state, { kind: "delta", authority, range: null, rows: [message] }).state;
  }
  const rows = transcriptRows(state);
  assert.deepEqual(rows.slice(-2).map(message => message.id), ["prompt", "answer"]);
  assert.equal(rows[rows.length - 1]?.content, "complete");
  assert.equal(new Set(rows.map(message => message.id)).size, rows.length);
});

test("a restarted delta ordinal evicts the oldest history row at the retention bound", () => {
  const authority = { sourceSessionId: "session", sessionBindingNonce: "binding" };
  const snapshot = reduceTranscript(createTranscriptState(), {
    kind: "snapshot", authority, range: null,
    rows: Array.from({ length: 440 }, (_, index) => row(`history-${index}`, index)),
  }).state;
  const { state } = reduceTranscript(snapshot, {
    kind: "delta", authority, range: null, rows: [row("new", 0)],
  });
  const rows = transcriptRows(state);
  assert.equal(rows.length, 440);
  assert.equal(rows[rows.length - 1]?.id, "new");
  assert.equal(rows[0]?.id, "history-1");
  assert.equal(rows.some(message => message.id === "history-0"), false);
  assert.equal(state.evictedOlder, 1);
});

test("a backfill batch precedes kept item rows despite reused local ordinals", () => {
  const authority = { sourceSessionId: "session", sessionBindingNonce: "binding" };
  const snapshot = reduceTranscript(createTranscriptState(), {
    kind: "snapshot", authority, range: { start: 3, end: 6, unit: "items" },
    rows: [row("recent-0", 0), row("recent-1", 1), row("recent-2", 2)],
  }).state;
  const result = reduceTranscript(snapshot, {
    kind: "backfill", authority, range: { start: 0, end: 3, unit: "items" },
    rows: [row("older-0", 0), row("older-1", 1), row("older-2", 2)],
  });
  assert.equal(result.outcome, "adopted");
  assert.deepEqual(transcriptRows(result.state).map(message => message.id), [
    "older-0", "older-1", "older-2", "recent-0", "recent-1", "recent-2",
  ]);
});

test("archived provider rows stay before current-session backfill pages", () => {
  const authority = { sourceSessionId: "current-session", sessionBindingNonce: "binding" };
  let state = reduceTranscript(createTranscriptState(), {
    kind: "snapshot",
    authority,
    range: { start: 100, end: 200 },
    rows: [row("current-tail", 1)],
  }).state;
  state = reduceTranscript(state, {
    kind: "snapshot",
    authority,
    range: null,
    placement: "oldest",
    rows: [row("previous-message", 1), row("handoff", 2)],
  }).state;

  const backfill = reduceTranscript(state, {
    kind: "backfill",
    authority,
    range: { start: 0, end: 100 },
    rows: [row("current-handoff", 0)],
  });
  assert.equal(backfill.outcome, "adopted");
  assert.deepEqual(transcriptRows(backfill.state).map(message => message.id), [
    "previous-message", "handoff", "current-handoff", "current-tail",
  ]);

  const refresh = reduceTranscript(backfill.state, {
    kind: "snapshot",
    authority,
    range: { start: 100, end: 200 },
    rows: [row("current-tail", 1, "refreshed")],
  });
  assert.deepEqual(transcriptRows(refresh.state).map(message => message.id), [
    "previous-message", "handoff", "current-handoff", "current-tail",
  ]);
});
