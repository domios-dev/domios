import assert from "node:assert/strict";
import { test } from "node:test";
import { createTranscriptState, reduceTranscript, projectTranscript, type NormalizedChatMessage } from "@codeterm/chat-core";
import { deliveryProjectionSends, queuedBehindTurn, type DeliverySend } from "./chatDeliveryModel";
import { createDeliveryProjectionMemory, queuedDeliveryIds, stableDeliveryEntries } from "./chatDeliveryProjection";
import type { EnrichedChatEntry } from "./chatEnrichment";

const memory = () => createDeliveryProjectionMemory("session");
const send: DeliverySend = { id: "local", correlationId: "token", content: "opaque", sentAtMs: 1_800_000_000_000 };
const entry = (msg: NormalizedChatMessage): EnrichedChatEntry => ({ msg, item: { kind: "message", message: msg }, origin: undefined });

test("a queued send stays after the running output through turn end, while steering uses send time", () => {
  const transcript = reduceTranscript(createTranscriptState(), { kind: "snapshot", authority: { sourceSessionId: "own", sessionBindingNonce: "launch" }, range: null,
    rows: [{ id: "output", type: "assistant", content: "streaming", timestamp: String(send.sentAtMs + 1000) }] }).state;
  for (const queue of [true, false]) {
    const state = memory();
    for (const [index, status] of ([undefined, "written", "submit_unconfirmed", "delivered"] as const).entries()) {
      const current: DeliverySend = status ? { ...send, status: { tab_id: "tab", send_id: "tx", correlationId: "token", state: status,
        sequence: index, ledger_id: "ledger", retained_from: 1, error: null } } : send;
      const queued = queuedDeliveryIds([current], queue, state);
      assert.deepEqual(projectTranscript(transcript, deliveryProjectionSends([current], null, queued)).rows.map(row => row.id),
        queue && status !== "delivered" ? ["output", "local"] : ["local", "output"]);
    }
  }
});

test("an explicit active-turn hold supplies queue placement without transport assumptions", () => {
  const state = memory();
  const held: DeliverySend = { ...send, status: { tab_id: "tab", send_id: "tx", state: "held", sequence: 1, ledger_id: "ledger", retained_from: 1,
    error: null, pending_reason: "held_active_turn" } };
  assert(queuedDeliveryIds([held], false, state).has(send.id));
  assert(queuedDeliveryIds([{ ...send, wasHeld: true }], false, state).has(send.id));
  assert.equal(queuedDeliveryIds([], false, state).size, 0);
});

test("exact mid-turn image evidence releases tail placement while the agent keeps working", () => {
  const authority = { sourceSessionId: "own", sessionBindingNonce: "launch" };
  const content = 'Inspect this image\nThen check the caption <domios type="attachment" kind="image" path="C:/chat/image.png" label="image1" />';
  const pending: DeliverySend = { ...send, content, echoBoundary: { ...authority, lastRowId: "before" } };
  const state = memory();
  assert(queuedDeliveryIds([pending], true, state).has(send.id));
  const delivered: DeliverySend = { ...pending, status: { tab_id: "tab", send_id: "tx", correlationId: "token",
    state: "delivered", sequence: 2, ledger_id: "ledger", retained_from: 1, error: null } };
  const queued = queuedDeliveryIds([delivered], true, state);
  assert.equal(queued.size, 0, "a delivered receipt ends the wait without waiting for idle");
  const rows: NormalizedChatMessage[] = [
    { id: "before", sourceSessionId: "own", type: "tool_result", content: "read complete" },
    { id: "mid-turn", sourceSessionId: "own", type: "user", content, timelineMarker: { kind: "queued" }, timestamp: String(send.sentAtMs + 100) },
    { id: "after", sourceSessionId: "own", type: "assistant", content: "image reviewed", timestamp: String(send.sentAtMs + 200) },
  ];
  const transcript = reduceTranscript(createTranscriptState(), { kind: "snapshot", authority, range: null, rows }).state;
  const projection = projectTranscript(transcript, deliveryProjectionSends([delivered], null, queued));
  assert.deepEqual(projection.pending, []);
  assert.deepEqual(projection.rows.map(row => row.id), ["before", "mid-turn", "after"]);
  const confirmed = { ...delivered, echo: projection.echoes.get(send.id) };
  assert.equal(queuedBehindTurn(confirmed, "working"), false);
  assert.equal(queuedDeliveryIds([confirmed], true, state).size, 0);
  assert.equal(queuedDeliveryIds([confirmed], false, state).size, 0);
});

test("a source echo takes over one local row identity without a blank handoff render", () => {
  const state = memory();
  const pending = entry({ id: send.id, type: "user", content: send.content, correlationId: send.correlationId, timestamp: String(send.sentAtMs) });
  assert.equal(stableDeliveryEntries([pending], [send], new Map(), state)[0], pending);
  const echo = { content: send.content, timestampMs: send.sentAtMs + 100 };
  const confirmed: DeliverySend = { ...send, echo, matchedRow: {
    source: { source_session_id: "own", record: { kind: "jsonl", incarnation: "file", record_start: 100, record_row: 0 } }, ordinal: 1,
  } };
  assert.equal(stableDeliveryEntries([], [confirmed], new Map(), state)[0], pending);
  const source = entry({ id: "source-segment-0", sourceMessageId: "source", sourceSessionId: "own", type: "user", content: send.content,
    timestamp: String(echo.timestampMs) });
  const foreign = entry({ ...source.msg, id: "foreign", sourceSessionId: "other" });
  assert.equal(stableDeliveryEntries([foreign], [confirmed], new Map(), memory())[0], foreign, "another session cannot take the local identity");
  const rows = stableDeliveryEntries([source], [confirmed], new Map(), state);
  assert.equal(rows.length, 1);
  assert.equal(rows[0].msg.id, send.id);
  assert.equal(rows[0].msg.sourceMessageId, "source");
  assert.equal(rows[0].msg.sourceSessionId, "own");
  assert.equal(state.pending.size, 0);
  assert.equal(stableDeliveryEntries([], [confirmed], new Map(), state).length, 0, "paging a committed row out never resurrects it");
});

test("dismissal and a cleared send set remove the retained handoff bubble", () => {
  for (const removed of [[], [{ ...send, dismissed: true }]]) {
    const state = memory();
    const pending = entry({ id: send.id, type: "user", content: send.content });
    stableDeliveryEntries([pending], [send], new Map(), state);
    assert.deepEqual(stableDeliveryEntries([], removed, new Map(), state), []);
    assert.equal(state.pending.size, 0);
  }
});
