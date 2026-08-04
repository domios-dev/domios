// Run: npx tsx packages/chat-core/src/transcript.test.ts
// Byte-range transcript authority. Every case is a projection the production
// bounded-snapshot seam produced or must never produce again.
import {
  createTranscriptState,
  projectTranscript,
  reduceTranscript,
  transcriptBackfillCursor,
  transcriptCommittedEnd,
  transcriptRows,
  type TranscriptBatch,
  type TranscriptState,
} from "./transcript";
import type { NormalizedChatMessage } from "./types";

let passed = 0;
let failed = 0;
function eq(actual: unknown, expected: unknown, name: string) {
  const a = JSON.stringify(actual);
  const e = JSON.stringify(expected);
  if (a === e) {
    passed += 1;
    console.log(`  ✓ ${name}`);
  } else {
    failed += 1;
    console.error(`  ✗ ${name}: expected ${e}, got ${a}`);
  }
}

const SESSION = { sourceSessionId: "sid-1", sessionBindingNonce: "nonce-1" };

/** Window-local ids/seq exactly as a bounded parse emits them: both restart at
 *  the window start, so they collide across independently parsed windows. */
function windowRows(
  contents: string[],
  type: NormalizedChatMessage["type"] = "assistant",
): NormalizedChatMessage[] {
  return contents.map((content, index) => ({
    id: `${index}-0`,
    type,
    content,
    seq: index,
    sourceSessionId: SESSION.sourceSessionId,
  }));
}

function batch(
  kind: TranscriptBatch["kind"],
  start: number,
  end: number,
  rows: NormalizedChatMessage[],
  authority = SESSION,
): TranscriptBatch {
  return { kind, authority, range: { start, end }, rows };
}

function contents(state: TranscriptState): string[] {
  return transcriptRows(state).map((row) => row.content);
}

// ── 1. Production windows A [820083374,823269577) and B [822788184,825799303):
// a shifted start with reused seq must not replace the prior rows.
{
  const first = reduceTranscript(
    createTranscriptState(),
    batch("snapshot", 820083374, 823269577, windowRows(["older", "prior answer"])),
  );
  const live = reduceTranscript(
    first.state,
    batch("delta", 823269577, 823269900, [
      { id: "823269577-0", type: "assistant", content: "live tail", seq: 0 },
    ]),
  );
  eq(live.outcome, "adopted", "contiguous live delta appends");
  const second = reduceTranscript(
    live.state,
    batch("snapshot", 822788184, 825799303, windowRows(["prior answer", "live tail", "new turn"])),
  );
  eq(second.outcome, "adopted", "advanced window adopts");
  eq(
    contents(second.state),
    ["prior answer", "live tail", "new turn"],
    "shifted window with reused seq keeps the prior answer and appends the new tail",
  );
  eq(transcriptCommittedEnd(second.state), 825799303, "committed end follows the newest window");
  eq(transcriptBackfillCursor(second.state), 822788184, "paging cursor is the newest window start");
}

// ── 1b. A window that advances past older committed segments keeps them.
{
  const older = reduceTranscript(
    createTranscriptState(),
    batch("snapshot", 0, 100, windowRows(["a", "b"])),
  );
  const advanced = reduceTranscript(
    older.state,
    batch("snapshot", 100, 300, windowRows(["c", "d"])),
  );
  eq(
    contents(advanced.state),
    ["a", "b", "c", "d"],
    "rows entirely below the new window start are preserved, not replaced",
  );
}

// ── 1c. A snapshot parsed before the newest bytes keeps the live tail that
// raced ahead of it, instead of being treated as a regression.
{
  const committed = reduceTranscript(
    createTranscriptState(),
    batch("snapshot", 0, 400, windowRows(["history"])),
  ).state;
  const racing = reduceTranscript(
    committed,
    batch("delta", 400, 460, [{ id: "400-0", type: "assistant", content: "raced ahead", seq: 0 }]),
  ).state;
  const late = reduceTranscript(racing, batch("snapshot", 0, 400, windowRows(["history", "while hidden"])));
  eq(late.outcome, "adopted", "a snapshot ending below the live tail still adopts");
  eq(
    contents(late.state),
    ["history", "while hidden", "raced ahead"],
    "the racing live tail survives the snapshot that predates it",
  );
}

// ── 2. Cached projection then the authoritative snapshot: one transcript.
{
  const cached = reduceTranscript(
    createTranscriptState(),
    batch("snapshot", 500, 900, windowRows(["turn one", "turn two"])),
  ).state;
  const authoritative = reduceTranscript(
    cached,
    batch("snapshot", 500, 1200, windowRows(["turn one", "turn two", "turn three"])),
  );
  eq(authoritative.outcome, "adopted", "authoritative snapshot over a cached window adopts");
  eq(
    contents(authoritative.state),
    ["turn one", "turn two", "turn three"],
    "cached window is superseded exactly — no alternate transcript, no duplicates",
  );
  const replayed = reduceTranscript(
    authoritative.state,
    batch("snapshot", 500, 1200, windowRows(["turn one", "turn two", "turn three"])),
  );
  eq(contents(replayed.state), contents(authoritative.state), "re-attach at the same range is a no-op");
}

// ── 3. Send while reattaching: optimistic user stays above its turn, once.
{
  const attached = reduceTranscript(
    createTranscriptState(),
    batch("snapshot", 0, 400, [
      { id: "0-0", type: "assistant", content: "earlier reply", seq: 0, clientOrder: 0 },
    ]),
  ).state;
  const optimistic: NormalizedChatMessage[] = [
    { id: "opt-1", type: "user", content: "run the build", timestamp: "1700000000000", clientOrder: 1 },
  ];
  const turn = reduceTranscript(
    attached,
    batch("delta", 400, 700, [
      { id: "400-0", type: "tool_call", content: "Bash build.sh", seq: 0, clientOrder: 2 },
    ]),
  ).state;
  eq(
    projectTranscript(turn, optimistic).rows.map((row) => row.content),
    ["earlier reply", "run the build", "Bash build.sh"],
    "optimistic user stays before the tool row its send triggered",
  );
  const echoed = reduceTranscript(
    turn,
    batch("delta", 700, 900, [
      { id: "700-0", type: "user", content: "run the build", seq: 0, clientOrder: 3 },
    ]),
  ).state;
  const reconciled = projectTranscript(echoed, optimistic);
  eq(reconciled.pending.length, 0, "the parsed echo reconciles the optimistic row");
  eq(
    reconciled.rows.filter((row) => row.content === "run the build").length,
    1,
    "reconciles exactly once — no duplicate user bubble",
  );
}

// ── 4. Live delta racing a snapshot: monotonic and idempotent by byte range.
{
  const base = reduceTranscript(
    createTranscriptState(),
    batch("snapshot", 0, 500, windowRows(["a"])),
  ).state;
  const replayed = reduceTranscript(base, batch("delta", 200, 500, windowRows(["a"])));
  eq(replayed.outcome, "ignored_stale", "a delta inside the committed span is idempotent");
  eq(contents(replayed.state), ["a"], "stale delta changes nothing");

  const straddling = reduceTranscript(base, batch("delta", 400, 700, windowRows(["a", "b"])));
  eq(straddling.outcome, "recover_overlap", "an unprovable partial overlap fails closed");
  eq(contents(straddling.state), ["a"], "unprovable overlap never duplicates rows");

  const gapped = reduceTranscript(base, batch("delta", 900, 1000, [
    { id: "900-0", type: "assistant", content: "newest", seq: 0 },
  ]));
  eq(gapped.outcome, "recover_gap", "a gapped delta asks for an authoritative snapshot");
  eq(
    contents(gapped.state),
    ["a"],
    "a committed transcript never grows a hole; the replacement snapshot restores the tail",
  );
}

// A source with no byte model has no gap to detect: rows merge by id and the
// newest one stays last, so a skipped id is not a hole.
{
  const unranged = (id: string, content: string): NormalizedChatMessage => ({
    id,
    type: "assistant",
    content,
  });
  const base = reduceTranscript(createTranscriptState(), {
    kind: "snapshot",
    authority: SESSION,
    range: null,
    rows: [unranged("db-1", "first")],
  }).state;
  const skipped = reduceTranscript(base, {
    kind: "delta",
    authority: SESSION,
    range: null,
    rows: [unranged("db-4", "newest")],
  });
  eq(skipped.outcome, "adopted", "an unranged delta is always adopted");
  eq(contents(skipped.state), ["first", "newest"], "skipped ids merge without loss");
  eq(
    transcriptRows(skipped.state)[transcriptRows(skipped.state).length - 1]?.id,
    "db-4",
    "the newest unranged row stays last",
  );
}

// Retention across three windows: whole oldest windows leave, the newest is
// never touched, and evictedOlder grows by exactly the rows removed.
{
  const win = (prefix: string, count: number) => Array.from({ length: count }, (_, index) => ({
    id: `${prefix}-${index}`,
    type: "assistant" as const,
    content: `${prefix}-${index}`,
    seq: index,
  }));
  const first = reduceTranscript(createTranscriptState(), batch("snapshot", 0, 300, win("a", 3)), 4);
  const second = reduceTranscript(first.state, batch("delta", 300, 600, win("b", 3)), 4);
  const third = reduceTranscript(second.state, batch("delta", 600, 900, win("c", 3)), 4);
  eq(
    contents(third.state),
    ["b-0", "b-1", "b-2", "c-0", "c-1", "c-2"],
    "only the oldest whole window is removed; the newest window survives intact",
  );
  eq(third.state.evictedOlder, 3, "evictedOlder grows by exactly the evicted window row count");
  eq(
    transcriptBackfillCursor(third.state),
    300,
    "the paging cursor moves to the oldest retained window start",
  );
}

// ── 5. The bound evicts whole parsed windows from the top, never the tail,
// and every evicted window is recoverable by exactly one page.
{
  const win = (prefix: string, count: number) => Array.from({ length: count }, (_, index) => ({
    id: `${prefix}-${index}`,
    type: "assistant" as const,
    content: `${prefix}-${index}`,
    seq: index,
  }));
  const first = reduceTranscript(createTranscriptState(), batch("snapshot", 100, 500, win("a", 4)), 4);
  const second = reduceTranscript(first.state, batch("delta", 500, 900, win("b", 4)), 4);
  eq(
    contents(second.state),
    ["b-0", "b-1", "b-2", "b-3"],
    "two whole windows over the bound evict the oldest entirely, never a slice",
  );
  const third = reduceTranscript(second.state, batch("delta", 900, 1_000, win("c", 1)), 4);
  eq(
    contents(third.state),
    ["b-0", "b-1", "b-2", "b-3", "c-0"],
    "the oldest whole window is evicted; the newest tail always stays",
  );
  eq(third.state.evictedOlder, 4, "evicted rows are counted for the hidden-history affordance");
  eq(
    transcriptBackfillCursor(third.state),
    500,
    "the paging cursor is the retained window's exact start, so no evicted row is stranded",
  );
  const recovered = reduceTranscript(
    third.state,
    { kind: "backfill", authority: SESSION, range: { start: 100, end: 500 }, rows: win("a", 4) },
    4,
  );
  eq(
    contents(recovered.state),
    ["a-0", "a-1", "a-2", "a-3", "b-0", "b-1", "b-2", "b-3", "c-0"],
    "one page ending at the cursor restores the evicted window exactly once",
  );
  eq(recovered.state.evictedOlder, 0, "recovering the evicted window clears the hidden count");
  const recoveredRows = contents(recovered.state);
  eq(
    recoveredRows[recoveredRows.length - 1],
    "c-0",
    "paging older never costs the newest live tail",
  );
}

// ── 5b. Ranged identity is derived from the batch range plus the row's
// position, so reused transport ids and seq cannot alias across batches.
{
  const base = reduceTranscript(
    createTranscriptState(),
    batch("snapshot", 0, 100, windowRows(["history"])),
  ).state;
  const live = (content: string): NormalizedChatMessage[] => [
    { id: "stream-7", type: "assistant", content, seq: 7, sourceSessionId: SESSION.sourceSessionId },
  ];
  const firstLive = reduceTranscript(base, batch("delta", 100, 200, live("first live"))).state;
  const secondLive = reduceTranscript(firstLive, batch("delta", 200, 300, live("second live")));
  eq(secondLive.outcome, "adopted", "a contiguous second live batch adopts");
  eq(
    contents(secondLive.state),
    ["history", "first live", "second live"],
    "two live batches reusing one transport id and seq stay distinct rows",
  );
  eq(
    transcriptRows(secondLive.state).map((row) => row.id),
    ["0:0", "100:0", "200:0"],
    "row identity comes from the authoritative batch range plus ordinal",
  );
  const replayed = reduceTranscript(
    secondLive.state,
    batch("delta", 200, 300, [
      { id: "stream-99", type: "assistant", content: "second live", seq: 0 },
    ]),
  );
  eq(replayed.outcome, "ignored_stale", "replaying a committed byte range is idempotent");
  eq(
    contents(replayed.state).length,
    3,
    "an idempotent replay dedupes even when the transport mints a new id",
  );
  const resnapshot = reduceTranscript(
    secondLive.state,
    batch("snapshot", 0, 300, [
      ...windowRows(["history"]),
      { id: "fresh-a", type: "assistant", content: "first live", seq: 0 },
      { id: "fresh-b", type: "assistant", content: "second live", seq: 1 },
    ]),
  );
  eq(
    contents(resnapshot.state),
    ["history", "first live", "second live"],
    "a re-parse of the same bytes projects the same rows once, not duplicates",
  );
}

// ── 6. Session rotation replaces the old session cleanly.
{
  const old = reduceTranscript(
    createTranscriptState(),
    batch("snapshot", 0, 400, windowRows(["old session"])),
  ).state;
  const rotated = reduceTranscript(
    old,
    batch("snapshot", 0, 200, windowRows(["new session"]), {
      sourceSessionId: "sid-2",
      sessionBindingNonce: "nonce-2",
    }),
  );
  eq(rotated.outcome, "rotated", "a different session authority rotates");
  eq(contents(rotated.state), ["new session"], "rotation replaces the previous transcript");
  eq(rotated.state.evictedOlder, 0, "rotation resets hidden-history accounting");
  eq(transcriptCommittedEnd(rotated.state), 200, "a rotated session may legally regress the offset");
}

// ── 7. Inverses: regressing and malformed ranges fail closed.
{
  const base = reduceTranscript(
    createTranscriptState(),
    batch("snapshot", 100, 900, windowRows(["committed"])),
  ).state;
  const regressed = reduceTranscript(base, batch("snapshot", 100, 500, windowRows(["stale window"])));
  eq(regressed.outcome, "rejected_range", "a snapshot regressing behind the committed end is refused");
  eq(contents(regressed.state), ["committed"], "a regressing snapshot cannot replace the transcript");

  const malformed = reduceTranscript(base, batch("snapshot", 900, 100, windowRows(["inverted"])));
  eq(malformed.outcome, "rejected_range", "an inverted range is refused");

  const fractional = reduceTranscript(base, {
    kind: "delta",
    authority: SESSION,
    range: { start: 900, end: Number.NaN },
    rows: windowRows(["nan"]),
  });
  eq(fractional.outcome, "rejected_range", "a non-finite range is refused");

  const overlapPage = reduceTranscript(base, {
    kind: "backfill",
    authority: SESSION,
    range: { start: 0, end: 500 },
    rows: windowRows(["overlapping page"]),
  });
  eq(overlapPage.outcome, "rejected_range", "a backfill page overlapping committed history is refused");

  const empty = reduceTranscript(base, batch("snapshot", 100, 2000, []));
  eq(empty.outcome, "ignored_empty", "an empty snapshot never wipes a committed transcript");
  eq(contents(empty.state), ["committed"], "empty snapshot keeps the transcript");
}

// ── Production duplicate-row RED: reattachment plus live adoption never
// projects a row twice, and never collapses repeated authored content.
{
  const authored = (id: string, seq: number, content: string): NormalizedChatMessage => ({
    id,
    type: "user",
    content,
    seq,
    sourceSessionId: SESSION.sourceSessionId,
  });
  const first = reduceTranscript(
    createTranscriptState(),
    batch("snapshot", 1000, 2000, [
      authored("a-0", 0, "continue"),
      authored("a-1", 1, "continue"),
      { ...authored("a-2", 2, "working on it"), type: "assistant" },
    ]),
  ).state;
  eq(
    contents(first).length,
    3,
    "two identical authored rows at distinct byte positions both survive",
  );
  // Reattach: same bytes, entirely new transport ids and the same seq values.
  const reattached = reduceTranscript(
    first,
    batch("snapshot", 1000, 2400, [
      authored("b-0", 0, "continue"),
      authored("b-1", 1, "continue"),
      { ...authored("b-2", 2, "working on it"), type: "assistant" },
      authored("b-3", 3, "continue"),
    ]),
  );
  eq(
    contents(reattached.state),
    ["continue", "continue", "working on it", "continue"],
    "reattachment with new transport ids projects each row once and keeps real repeats",
  );
  // The live tailer re-delivers bytes the reattach snapshot already covered.
  const replayed = reduceTranscript(
    reattached.state,
    batch("delta", 2000, 2400, [authored("c-3", 0, "continue")]),
  );
  eq(replayed.outcome, "ignored_stale", "a re-delivered committed range is refused by byte range");
  eq(
    contents(replayed.state).length,
    4,
    "re-delivery with a different transport id cannot duplicate a committed row",
  );
  const sent = reduceTranscript(
    reattached.state,
    batch("delta", 2400, 2600, [authored("d-0", 0, "continue")]),
  );
  eq(
    contents(sent.state).length,
    5,
    "a genuinely new send of the same text is adopted, not deduped away",
  );
}

// ── Unranged sources (structured/store-backed) keep id-merge semantics.
{
  const first = reduceTranscript(createTranscriptState(), {
    kind: "snapshot",
    authority: SESSION,
    range: null,
    rows: [{ id: "db-1", type: "assistant", content: "hello" }],
  });
  const streamed = reduceTranscript(first.state, {
    kind: "delta",
    authority: SESSION,
    range: null,
    rows: [
      { id: "db-1", type: "assistant", content: "hello world" },
      { id: "db-2", type: "assistant", content: "second" },
    ],
  });
  eq(
    contents(streamed.state),
    ["hello world", "second"],
    "unranged providers still merge in place by stable id",
  );
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
