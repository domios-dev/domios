import assert from "node:assert/strict";
import { test } from "node:test";
import type { ChatSend } from "./sendOrder";
import { createTranscriptState, projectTranscript, reduceTranscript, type TranscriptState } from "./transcript";
import type { NormalizedChatMessage } from "./types";

const authority = { sourceSessionId: "session-a", sessionBindingNonce: "launch-a" };
const at = 1_794_840_000_000;
const row = (id: string, offsetMs: number, type: NormalizedChatMessage["type"] = "assistant"): NormalizedChatMessage => ({
  id, type, content: `opaque ${id}`, timestamp: new Date(at + offsetMs).toISOString(),
});
const claim = (id: string, itemId: string, sentOffsetMs: number, session = "session-a"): ChatSend => ({
  id, content: `opaque ${itemId}`, sentAtMs: at + sentOffsetMs, flowAtMs: at + sentOffsetMs,
  matchedRow: { source: { source_session_id: session, record: {
    kind: "structured", thread_id: session, item_id: itemId,
  } }, ordinal: 1 },
});
const window = (start: number, rows: NormalizedChatMessage[]): TranscriptState => reduceTranscript(createTranscriptState(), {
  kind: "snapshot", authority, range: { start, end: start + 4_096 }, rows,
}).state;
const newest = [row("loaded-reply", 5_000), row("loaded-owner", 6_000, "user"), row("latest-reply", 7_000)];
const ids = (state: TranscriptState, sends: ChatSend[]) => projectTranscript(state, sends).rows.map(entry => entry.id);

test("delivered claims recorded before a partial window stay settled in unloaded history, not at its tail", () => {
  const sends = [claim("send-early", "item-early", 1_000), claim("send-later", "item-later", 2_000)];
  const projection = projectTranscript(window(8_192, newest), sends);

  assert.deepEqual(projection.rows.map(entry => entry.id), ["loaded-reply", "loaded-owner", "latest-reply"]);
  assert.deepEqual(projection.pending, []);
  assert.deepEqual([...projection.outside], ["send-early", "send-later"]);
});

test("paging the claimed records in shows each once, in order, where it was recorded", () => {
  const sends = [claim("send-early", "item-early", 1_000), claim("send-later", "item-later", 2_000)];
  const paged = reduceTranscript(window(8_192, newest), {
    kind: "backfill", authority, range: { start: 4_096, end: 8_192 },
    rows: [row("item-early", 1_000, "user"), row("between", 1_500), row("item-later", 2_000, "user")],
  }).state;
  const projection = projectTranscript(paged, sends);

  assert.deepEqual(projection.rows.map(entry => entry.id),
    ["item-early", "between", "item-later", "loaded-reply", "loaded-owner", "latest-reply"]);
  assert.deepEqual(projection.pending, []);
  assert.deepEqual([...projection.outside], []);
  assert.deepEqual([...projection.echoes.keys()], ["send-early", "send-later"]);
});

test("a claim inside the loaded span has a provisional slot while it waits for its record", () => {
  assert.deepEqual(ids(window(8_192, newest), [claim("send-new", "item-new", 6_500)]),
    ["loaded-reply", "loaded-owner", "send-new", "latest-reply"]);
  assert.deepEqual(ids(window(0, newest), [claim("send-early", "item-early", 1_000)]),
    ["send-early", "loaded-reply", "loaded-owner", "latest-reply"]);
  const foreign = projectTranscript(window(8_192, newest), [claim("send-foreign", "item-foreign", 1_000, "session-b")]);
  assert.deepEqual(foreign.pending.map(send => send.id), ["send-foreign"]);
  assert.deepEqual([...foreign.outside], []);
});

test("delivered claims before an untimed partial items window stay settled in unloaded history, not at the tail", () => {
  const untimedTail: NormalizedChatMessage[] = Array.from({ length: 200 }, (_, offset) => ({
    id: `acp-${1281 + offset}`,
    type: offset % 3 === 0 ? "thinking" : offset % 3 === 1 ? "tool_call" : "assistant",
    content: offset === 199 ? "SYNTH_A_PHASE2_GROWTH_A_C0450" : `acp-row-${1281 + offset}`,
    seq: 1281 + offset,
  }));
  const partialItems = reduceTranscript(createTranscriptState(), {
    kind: "snapshot",
    authority,
    range: { unit: "items", start: 1281, end: 1481 },
    rows: untimedTail,
  }).state;
  const phase1 = claim("02faf3d6", "02faf3d6", 1_000);
  const phase2 = claim("711242a2", "711242a2", 2_000);

  const tailProjection = projectTranscript(partialItems, [phase1, phase2]);
  assert.deepEqual(tailProjection.pending, []);
  assert.deepEqual([...tailProjection.outside], ["02faf3d6", "711242a2"]);
  assert.equal(tailProjection.rows[tailProjection.rows.length - 1]?.id, "acp-1480");

  // A brand-new send issued after a loaded row in that untimed window still keeps its tail slot.
  const fresh: ChatSend = {
    ...claim("fresh-send", "fresh-item", 9_000),
    echoBoundary: { sourceSessionId: "session-a", sessionBindingNonce: "launch-a", lastRowId: "acp-1480" },
  };
  const withFresh = projectTranscript(partialItems, [phase1, phase2, fresh]);
  assert.deepEqual(withFresh.pending.map(send => send.id), ["fresh-send"]);
  assert.deepEqual([...withFresh.outside], ["02faf3d6", "711242a2"]);
  assert.equal(withFresh.rows[withFresh.rows.length - 1]?.id, "fresh-send");

  // Backfilling seq=0..1280 restores PHASE1 (seq=0) and PHASE2 (seq=126) in exact chronological order.
  const older: NormalizedChatMessage[] = [
    { ...row("02faf3d6", 1_000, "user"), seq: 0, content: "PHASE1_SMALL_A" },
    ...Array.from({ length: 125 }, (_, i) => ({ id: `acp-${i + 1}`, type: "assistant" as const, content: `r-${i + 1}`, seq: i + 1 })),
    { ...row("711242a2", 2_000, "user"), seq: 126, content: "PHASE2_GROWTH_A" },
    ...Array.from({ length: 1154 }, (_, i) => ({ id: `acp-${i + 127}`, type: "assistant" as const, content: `r-${i + 127}`, seq: i + 127 })),
  ];
  const backfilled = reduceTranscript(partialItems, {
    kind: "backfill",
    authority,
    range: { unit: "items", start: 0, end: 1281 },
    rows: older,
  }).state;
  const full = projectTranscript(backfilled, [phase1, phase2]);
  assert.deepEqual(full.pending, []);
  assert.deepEqual([...full.outside], []);
  assert.equal(full.rows[0]?.id, "02faf3d6");
  assert.equal(full.rows[126]?.id, "711242a2");
  assert.equal(full.rows[full.rows.length - 1]?.id, "acp-1480");
});

test("evicted older untimed unranged rows never resurrect older delivered claims at the tail", () => {
  const evictedState: TranscriptState = {
    authority,
    segments: [{
      kind: "delta",
      range: null,
      rows: [{ id: "acp-tail", type: "assistant", content: "SYNTH_A_PHASE2_GROWTH_A_C0450", sourceSessionId: "session-a", seq: 1480 }],
    }],
    evictedOlder: 1041,
  };
  const projection = projectTranscript(evictedState, [claim("02faf3d6", "02faf3d6", 1_000)]);
  assert.deepEqual(projection.pending, []);
  assert.deepEqual([...projection.outside], ["02faf3d6"]);
  assert.deepEqual(projection.rows.map(item => item.id), ["acp-tail"]);
});
