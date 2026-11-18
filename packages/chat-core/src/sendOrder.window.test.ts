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
