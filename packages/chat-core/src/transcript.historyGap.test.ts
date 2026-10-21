import { createTranscriptState, expandTranscriptGap, reduceTranscript, transcriptRows } from "./transcript";
import type { NormalizedChatMessage } from "./types";
import assert from "node:assert/strict";
const SESSION = { sourceSessionId: "sid-1", sessionBindingNonce: "nonce-1" };
function eq(actual: unknown, expected: unknown, name: string) {
  assert.deepEqual(actual, expected, name);
  console.log(`  ✓ ${name}`);
}
const row = (id: string): NormalizedChatMessage => ({ id, type: "assistant", content: id });
const gap = (id: string, before: number): NormalizedChatMessage => ({
  id, type: "system", content: "gap",
  timelineMarker: { kind: "history_gap", source: "provider_session", session_id: "archived", before_offset: before },
});

{
  const archived = reduceTranscript(createTranscriptState(), {
    kind: "snapshot", authority: SESSION, range: null, placement: "oldest",
    rows: [gap("gap-100", 100), row("old-1"), row("boundary")],
  });
  const current = reduceTranscript(archived.state, {
    kind: "snapshot", authority: SESSION, range: { start: 0, end: 10 }, rows: [row("now-1")],
  });
  const expanded = expandTranscriptGap(current.state, "gap-100", [gap("gap-40", 40), row("older-1"), row("older-2")]);
  eq(expanded.outcome, "adopted", "a known gap row expands");
  eq(transcriptRows(expanded.state).map((message) => message.id),
    ["gap-40", "older-1", "older-2", "old-1", "boundary", "now-1"],
    "the older page lands where the gap stood, before the archived rows and the current session");

  const resent = reduceTranscript(expanded.state, {
    kind: "snapshot", authority: SESSION, range: null, placement: "oldest",
    rows: [gap("gap-100", 100), row("old-1"), row("boundary")],
  });
  eq(transcriptRows(resent.state).map((message) => message.id).includes("gap-100"), false,
    "a re-sent copy of an expanded gap stays hidden");

  eq(expandTranscriptGap(expanded.state, "gap-100", [row("x")]).outcome, "ignored_stale", "an expanded gap does not expand twice");
  eq(expandTranscriptGap(expanded.state, "missing", [row("x")]).outcome, "ignored_stale", "an unknown gap id changes nothing");

  const rotated = reduceTranscript(expanded.state, {
    kind: "snapshot", authority: { sourceSessionId: "sid-2", sessionBindingNonce: "nonce-2" }, range: null, placement: "oldest",
    rows: [gap("gap-100", 100)],
  });
  eq(transcriptRows(rotated.state).map((message) => message.id), ["gap-100"], "a new authority starts with no retired gaps");
}

{
  const live = (ids: string[], first: NormalizedChatMessage[] = []) => {
    let state = reduceTranscript(createTranscriptState(), {
      kind: "snapshot", authority: SESSION, range: null, rows: [...first, row("s1"), row("s2")],
    }, 3).state;
    for (const id of ids) {
      state = reduceTranscript(state, { kind: "delta", authority: SESSION, range: null, rows: [row(id)] }, 3).state;
    }
    return transcriptRows(state).map((message) => message.id);
  };
  eq(live(["d1", "d2"]), ["s2", "d1", "d2"], "without a gap the bound trims the oldest unranged rows");
  eq(live(["d1", "d2"], [gap("gap-9", 9)]), ["gap-9", "s1", "s2", "d1", "d2"],
    "a segment led by a gap keeps every row, so its gap stays reachable");

  const archived = reduceTranscript(createTranscriptState(), {
    kind: "snapshot", authority: SESSION, range: null, placement: "oldest", rows: [gap("gap-5", 5), row("a1")],
  }, 3).state;
  const paged = expandTranscriptGap(archived, "gap-5", [row("p1"), row("p2"), row("p3")]).state;
  const grown = reduceTranscript(paged, {
    kind: "snapshot", authority: SESSION, range: { start: 0, end: 4 }, rows: [row("c1"), row("c2"), row("c3")],
  }, 3).state;
  eq(transcriptRows(reduceTranscript(grown, {
    kind: "delta", authority: SESSION, range: { start: 4, end: 8 }, rows: [row("c4")],
  }, 3).state).map((message) => message.id).slice(0, 4), ["p1", "p2", "p3", "a1"],
  "a fully paged archive is never evicted by current-session growth");
}

console.log("9 passed, 0 failed");
