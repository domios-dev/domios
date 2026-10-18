import assert from "node:assert/strict";
import { test } from "node:test";
import { captureSendEchoBoundary, type ChatSend } from "./sendOrder";
import { createTranscriptState, projectTranscript, reduceTranscript, transcriptRows, type TranscriptState } from "./transcript";
import type { NormalizedChatMessage } from "./types";

const authority = { sourceSessionId: "own-session", sessionBindingNonce: "own-launch" };
const serverAt = 1_792_250_000_000;
const content = "plain mobile message";
const user = (id: string): NormalizedChatMessage => ({ id, type: "user", content, timestamp: String(serverAt) });
const append = (state: TranscriptState, rows: NormalizedChatMessage[]) => reduceTranscript(state, {
  kind: "delta", authority, range: null, rows,
}).state;
const history = append(createTranscriptState(), [user("history")]);
const send = (id: string, state = history): ChatSend => ({
  id, correlationId: id, content, sentAtMs: serverAt + 30_000,
  echoBoundary: captureSendEchoBoundary(state.authority, transcriptRows(state)),
});

test("a phone clock 30 seconds ahead reconciles its untokened appended echo", () => {
  const result = projectTranscript(append(history, [user("echo")]), [send("send-a")]);
  assert.deepEqual(result.rows.map((row) => row.id), ["history", "echo"]);
  assert.equal(result.pending.length, 0);
  assert.equal(result.echoes.size, 1);
});

test("distinct identical sends consume distinct appended user rows", () => {
  const sends = [send("send-a"), send("send-b")];
  const first = projectTranscript(append(history, [user("echo-a")]), sends);
  assert.deepEqual(first.pending.map((row) => row.id), ["send-b"]);
  assert.equal(first.rows.filter((row) => row.id !== "history").length, 2);
  const both = projectTranscript(append(history, [user("echo-a"), user("echo-b")]), sends);
  assert.deepEqual(both.rows.map((row) => row.id), ["history", "echo-a", "echo-b"]);
  assert.equal(both.pending.length, 0);
  assert.equal(both.echoes.size, 2);
});

test("identical pre-send history cannot match even when its clock is newer", () => {
  const result = projectTranscript(history, [{ ...send("send-a"), sentAtMs: serverAt - 30_000 }]);
  assert.deepEqual(result.pending.map((row) => row.id), ["send-a"]);
  assert.equal(result.echoes.size, 0);
});

test("a send never reconciles against another bound session or launch", () => {
  for (const replacement of [
    { sourceSessionId: "other-session", sessionBindingNonce: "own-launch" },
    { sourceSessionId: "own-session", sessionBindingNonce: "other-launch" },
  ]) {
    const rotated = reduceTranscript(history, { kind: "delta", authority: replacement, range: null,
      rows: [{ ...user("foreign"), correlationId: "send-a" }] }).state;
    const result = projectTranscript(rotated, [send("send-a")]);
    assert.deepEqual(result.pending.map((row) => row.id), ["send-a"]);
    assert.equal(result.echoes.size, 0);
  }
});

test("an inherited user row cannot be an appended echo", () => {
  const result = projectTranscript(append(history, [{ ...user("inherited"), inheritedFromSessionId: "ancestor" }]), [send("send-a")]);
  assert.equal(result.pending.length, 1);
});

test("an unavailable append boundary cannot match a timestamp-compatible row", () => {
  const result = projectTranscript(append(createTranscriptState(), [user("unrelated")]), [
    { ...send("send-a"), sentAtMs: serverAt },
  ]);
  assert.equal(result.pending.length, 1);
});

test("an authoritative empty transcript supplies an empty append boundary", () => {
  const empty: TranscriptState = { ...createTranscriptState(), authority };
  const result = projectTranscript(append(empty, [user("first-echo")]), [send("send-a", empty)]);
  assert.deepEqual(result.rows.map((row) => row.id), ["first-echo"]);
  assert.equal(result.pending.length, 0);
  assert.equal(captureSendEchoBoundary(null, []), undefined);
});

test("recorded echoes preserve the boundary after store hydration", () => {
  const state = append(history, [user("echo")]);
  const original = send("send-a");
  const first = projectTranscript(state, [original]);
  const restored: ChatSend = JSON.parse(JSON.stringify({ ...original, echo: first.echoes.get(original.id) }));
  assert.equal(projectTranscript(state, [restored]).pending.length, 0);
  const rebound = { ...state, authority: { ...authority, sessionBindingNonce: "new-launch" } };
  assert.equal(projectTranscript(rebound, [restored]).pending.length, 0);
  assert.equal(projectTranscript(rebound, [restored]).rows.length, 2);
});

test("an appended row owned by another session is never an echo", () => {
  const result = projectTranscript(append(history, [{ ...user("foreign"), sourceSessionId: "other-session" }]), [send("send-a")]);
  assert.equal(result.pending.length, 1);
});

test("a structured matched row reconciles without content or timestamp matching", () => {
  const row = { ...user("native-item"), content: "provider-enriched", timestamp: String(serverAt - 60_000) };
  const state = append(history, [row]);
  const request: ChatSend = {
    ...send("send-a"),
    content: "composer text",
    matchedRow: { source: { source_session_id: "own-session", record: {
      kind: "structured", thread_id: "own-session", item_id: "native-item",
    } }, ordinal: 1 },
  };

  const result = projectTranscript(state, [request]);

  assert.deepEqual(result.rows.map((message) => message.id), ["history", "native-item"]);
  assert.equal(result.pending.length, 0);
  assert.equal(result.echoes.size, 1);
});

test("a persisted structured claim reconciles after the same session is rebound", () => {
  const rebound = append(history, [user("native-item")]);
  const state = { ...rebound, authority: { ...authority, sessionBindingNonce: "rebound-launch" } };
  const request: ChatSend = {
    ...send("send-a"),
    matchedRow: { source: { source_session_id: "own-session", record: {
      kind: "structured", thread_id: "own-session", item_id: "native-item",
    } }, ordinal: 1 },
  };

  const result = projectTranscript(state, [request]);

  assert.deepEqual(result.rows.map((row) => row.id), ["history", "native-item"]);
  assert.equal(result.pending.length, 0);
  assert.equal(result.echoes.size, 1);
});

test("intentional identical sends reconcile to their distinct structured rows", () => {
  const state = append(history, [user("native-a"), user("native-b")]);
  const matched = (itemId: string) => ({ source: { source_session_id: "own-session", record: {
    kind: "structured" as const, thread_id: "own-session", item_id: itemId,
  } }, ordinal: 1 });
  const requests = [
    { ...send("send-a"), matchedRow: matched("native-a") },
    { ...send("send-b"), matchedRow: matched("native-b") },
  ];

  const result = projectTranscript(state, requests);

  assert.deepEqual(result.rows.map((message) => message.id), ["history", "native-a", "native-b"]);
  assert.equal(result.pending.length, 0);
  assert.equal(result.echoes.size, 2);
});

test("a structured matched row from another session cannot claim the current transcript", () => {
  const state = append(history, [{ ...user("foreign-item"), sourceSessionId: "other-session" }]);
  const request: ChatSend = {
    ...send("send-a"),
    matchedRow: { source: { source_session_id: "other-session", record: {
      kind: "structured", thread_id: "other-session", item_id: "foreign-item",
    } }, ordinal: 1 },
  };

  const result = projectTranscript(state, [request]);

  assert.deepEqual(result.pending.map((item) => item.id), ["send-a"]);
  assert.equal(result.echoes.size, 0);
});

test("an exact old-session claim keeps its history row visible after handoff", () => {
  const state: TranscriptState = {
    authority: { sourceSessionId: "new-session", sessionBindingNonce: "new-launch" },
    segments: [{ kind: "snapshot", range: null, rows: [{
      ...user("old-item"), sourceSessionId: "own-session",
    }] }],
    evictedOlder: 0,
  };
  const request: ChatSend = {
    ...send("old-send"),
    matchedRow: { source: { source_session_id: "own-session", record: {
      kind: "structured", thread_id: "own-session", item_id: "old-item",
    } }, ordinal: 1 },
  };

  const result = projectTranscript(state, [request]);

  assert.deepEqual(result.rows.map((row) => row.id), ["old-item"]);
  assert.equal(result.pending.length, 0);
  assert.equal(result.echoes.size, 1);
});

test("a current send cannot claim matching old-session history after handoff", () => {
  const state: TranscriptState = {
    authority: { sourceSessionId: "new-session", sessionBindingNonce: "new-launch" },
    segments: [{ kind: "snapshot", range: null, rows: [{
      ...user("old-item"), sourceSessionId: "own-session",
    }] }],
    evictedOlder: 0,
  };
  const request: ChatSend = {
    ...send("new-send"),
    echoBoundary: { sourceSessionId: "new-session", sessionBindingNonce: "new-launch", lastRowId: null },
    matchedRow: { source: { source_session_id: "new-session", record: {
      kind: "structured", thread_id: "new-session", item_id: "old-item",
    } }, ordinal: 1 },
  };

  const result = projectTranscript(state, [request]);

  assert.deepEqual(result.pending.map((row) => row.id), ["new-send"]);
  assert.equal(result.echoes.size, 0);
});

test("clock-independent matching requires exact text", () => {
  const result = projectTranscript(append(history, [{ ...user("similar"), content: `${content} extra words` }]), [send("send-a")]);
  assert.equal(result.pending.length, 1);
});
