// Run: npx tsx packages/chat-core/src/merge.test.ts
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

// ── codex: ISO-string timestamps. The old comparator did Number("2026-..") = NaN
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

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
