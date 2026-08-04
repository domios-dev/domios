import { mergeById } from "./merge";
import type { NormalizedChatMessage } from "./types";

let passed = 0;
let failed = 0;
function eq(actual: unknown, expected: unknown, name: string) {
  const a = JSON.stringify(actual);
  const e = JSON.stringify(expected);
  if (a === e) {
    passed++;
    console.log(`  ✓ ${name}`);
  } else {
    failed++;
    console.error(`  ✗ ${name}: expected ${e}, got ${a}`);
  }
}

type M = NormalizedChatMessage;
function msg(id: string, opts: Partial<M> = {}): M {
  return { id, type: "user", content: id, ...opts };
}

//    → no real sort, leaving order to chance. (timestamp,seq) must hold send order.
{
  const incoming = [
    msg("u1", { type: "user", timestamp: "2026-06-20T17:42:57.501Z", seq: 1 }),
    msg("a1", { type: "assistant", timestamp: "2026-06-20T17:42:58.795Z", seq: 2 }),
    msg("u2", { type: "user", timestamp: "2026-06-20T17:42:58.835Z", seq: 3 }),
    msg("a2", { type: "assistant", timestamp: "2026-06-20T17:43:01.327Z", seq: 4 }),
  ];
  // Feed shuffled to prove the sort — not insertion luck — produces order.
  const merged = mergeById([], [incoming[2], incoming[0], incoming[3], incoming[1]]);
  eq(merged.map((m) => m.id), ["u1", "a1", "u2", "a2"], "codex ISO timestamps order by time");
}

// ── codex same-second: equal coarse timestamps fall back to seq (source order).
{
  const ts = "2026-06-20T17:42:58Z"; // second resolution: user + reply collide
  const merged = mergeById(
    [],
    [
      msg("a", { type: "assistant", timestamp: ts, seq: 5 }),
      msg("u", { type: "user", timestamp: ts, seq: 4 }),
    ],
  );
  eq(merged.map((m) => m.id), ["u", "a"], "same-second timestamps tiebreak by seq");
}

// ── cursor: NO timestamps at all. Ordering rests entirely on seq.
{
  const merged = mergeById(
    [],
    [
      msg("m3", { seq: 3 }),
      msg("m1", { seq: 1 }),
      msg("m5", { seq: 5 }),
      msg("m2", { seq: 2 }),
      msg("m4", { seq: 4 }),
    ],
  );
  eq(merged.map((m) => m.id), ["m1", "m2", "m3", "m4", "m5"], "timeless cursor msgs order by seq");
}

// ── streaming: snapshot then a later delta whose seq continues past it.
{
  const snapshot = [msg("s0", { seq: 0 }), msg("s1", { seq: 1 })];
  const delta = [msg("s2", { seq: 2 }), msg("s3", { seq: 3 })];
  const merged = mergeById(snapshot, delta);
  eq(merged.map((m) => m.id), ["s0", "s1", "s2", "s3"], "delta appends in seq order after snapshot");
}

// ── dedupe-in-place: a re-emitted id updates content and keeps its slot.
{
  const merged = mergeById(
    [msg("x", { seq: 0, content: "old" }), msg("y", { seq: 1 })],
    [msg("x", { seq: 0, content: "new" })],
  );
  eq(merged.map((m) => `${m.id}:${m.content}`), ["x:new", "y:y"], "same id updates in place, no dup");
}

// ── no seq, no timestamp (e.g. structured providers): preserve arrival order.
{
  const merged = mergeById([], [msg("a"), msg("b"), msg("c")]);
  eq(merged.map((m) => m.id), ["a", "b", "c"], "no signals → stable arrival order");
}

// ── c73becde launch race: chat_start snapshot and tab-chat-msg live event
// have separate transport ids but share the parser's user sequence.
{
  const startupText = "You are Theo. You are a frontend worker fixing a transcript projection/dedup defect.";
  const merged = mergeById(
    [msg("snapshot-user-1", { content: startupText, seq: 1, sourceSessionId: "c73becde" })],
    [msg("live-user-1", { content: startupText, seq: 1, sourceSessionId: "c73becde" })],
  );
  eq(merged.map((m) => m.id), ["live-user-1"], "snapshot/live user seq-1 projects one launch bubble");
}

{
  const stableId = "10c35d49-66da-4a15-ba9c-2fbfa1a8b8a7-0";
  const merged = mergeById(
    [msg(stableId, { content: "круто запусти еще раз", seq: 1647, sourceSessionId: "ab7dc0bb" })],
    [msg(stableId, { content: "круто запусти еще раз", seq: 1648, sourceSessionId: "ab7dc0bb" })],
  );
  eq(merged.map((m) => m.id), [stableId], "tab reactivation dedupes one provider UUID across shifted parser seq");
}

{
  const reply = "I traced the merge path and will fix the source identity.";
  const merged = mergeById(
    [msg("snapshot-assistant-7", { type: "assistant", content: reply, seq: 7, sourceSessionId: "c73becde" })],
    [msg("live-assistant-7", { type: "assistant", content: reply, seq: 7, sourceSessionId: "c73becde" })],
  );
  eq(merged.map((m) => m.id), ["live-assistant-7"], "snapshot/live assistant seq-7 projects one reply bubble");
}

{
  const merged = mergeById(
    [msg("tool-a", { type: "tool_call", seq: 7, sourceSessionId: "c73becde", toolId: "a" })],
    [msg("tool-b", { type: "tool_call", seq: 7, sourceSessionId: "c73becde", toolId: "b" })],
  );
  eq(merged.map((m) => m.id), ["tool-a", "tool-b"], "same-source tool blocks retain distinct identities");
}

// ── text is not identity: distinct parser sequences preserve true repeats.
{
  const merged = mergeById(
    [],
    [msg("repeat-1", { content: "continue", seq: 2, sourceSessionId: "c73becde" }), msg("repeat-2", { content: "continue", seq: 3, sourceSessionId: "c73becde" })],
  );
  eq(merged.map((m) => m.id), ["repeat-1", "repeat-2"], "distinct parser sequences preserve identical user text");
}

{
  const merged = mergeById(
    [msg("old-session", { content: "continue", seq: 1, sourceSessionId: "old" })],
    [msg("new-session", { content: "continue", seq: 1, sourceSessionId: "new" })],
  );
  eq(merged.map((m) => m.id), ["old-session", "new-session"], "session scope preserves a later seq reset");
}

{
  const incoming = [
    msg("ctx", { type: "user", timestamp: "2026-06-21T08:35:53.795Z", seq: 0 }),
    msg("u1", { type: "user", timestamp: "2026-06-21T08:35:53.827Z", seq: 1 }),
    msg("a1", { type: "assistant", timestamp: "2026-06-21T08:35:57.692Z", seq: 2 }),
    msg("u2", { type: "user", timestamp: "2026-06-21T08:42:05.097Z", seq: 3 }),
    msg("a2", { type: "assistant", timestamp: "2026-06-21T08:42:06.765Z", seq: 4 }),
  ];
  const merged = mergeById([], [incoming[3], incoming[1], incoming[4], incoming[0], incoming[2]]);
  eq(
    merged.map((m) => m.id),
    ["ctx", "u1", "a1", "u2", "a2"],
    "codex real ms-precision rollout: user always before its assistant reply",
  );
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
