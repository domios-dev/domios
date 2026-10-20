import assert from "node:assert/strict";
import { test } from "node:test";
import { createTranscriptState, reduceTranscript, reduceToolEvents, transcriptRows } from "@codeterm/chat-core";
import { chatWindowProjection, olderChatAction, slideChatWindowOlder, timelineItemKey } from "./chatWindow";

for (const unit of ["bytes", "items"] as const) {
  test(`${unit}: click requests one page, merged history projects into the older window`, () => {
    const authority = { sourceSessionId: "own-session", sessionBindingNonce: "own-nonce" };
    const rows = (start: number, end: number) => Array.from({ length: end - start }, (_, i) => ({
      id: `row-${start + i}`, type: "assistant" as const, content: `answer-${start + i}`,
    }));
    const snapshot = reduceTranscript(createTranscriptState(), { kind: "snapshot", authority,
      range: { start: 220, end: 440, unit }, rows: rows(220, 440) }).state;
    const before = reduceToolEvents(transcriptRows(snapshot));
    assert.deepEqual(olderChatAction(before, "row-439", true, 220, false), { kind: "backfill", beforeOffset: 220 });
    assert.deepEqual(olderChatAction(before, "row-439", true, 220, true), { kind: "none" });
    const merged = reduceTranscript(snapshot, { kind: "backfill", authority,
      range: { start: 0, end: 220, unit }, rows: rows(0, 220) });
    assert.equal(merged.outcome, "adopted");
    const after = reduceToolEvents(transcriptRows(merged.state));
    const anchor = slideChatWindowOlder(after, "row-439");
    const visible = chatWindowProjection(after, anchor).items;
    assert.equal(visible.length, 220);
    assert.equal(timelineItemKey(visible[0]), "row-60");
    assert.deepEqual(olderChatAction(after, anchor, true, 0, false), { kind: "reveal", anchorKey: "row-219" });
    assert.deepEqual(olderChatAction(after, "row-219", true, 0, false), { kind: "none" });
  });
}

test("missing cursor and unloaded anchor cannot fabricate history", () => {
  assert.deepEqual(olderChatAction([], null, false, null, false), { kind: "none" });
});
