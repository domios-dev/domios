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

test("clock-independent matching requires exact text", () => {
  const result = projectTranscript(append(history, [{ ...user("similar"), content: `${content} extra words` }]), [send("send-a")]);
  assert.equal(result.pending.length, 1);
});
