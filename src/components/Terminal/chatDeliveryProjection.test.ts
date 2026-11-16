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
        queue ? ["output", "local"] : ["local", "output"]);
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
  assert.equal(queued.size, 1, "a receipt without a located record keeps the tail until its echo");
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
    sourceRecord: confirmed.matchedRow!.source.record,
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

test("a retained handoff bubble whose record lies before the loaded window leaves the tail", () => {
  const state = memory();
  const pending = entry({ id: send.id, type: "user", content: send.content });
  stableDeliveryEntries([pending], [send], new Map(), state);
  const latest = entry({ id: "latest", type: "assistant", content: "opaque latest" });
  assert.deepEqual(stableDeliveryEntries([latest], [send], new Map(), state, new Set([send.id])).map(row => row.msg.id), ["latest"]);
  assert.equal(state.pending.size, 0);
});

test("a delivered record moves from the tail to its final slot once before a late echo", () => {
  const authority = { sourceSessionId: "own", sessionBindingNonce: "launch" };
  const record = (offset: number) => ({ kind: "jsonl" as const, incarnation: "file", record_start: offset, record_row: 0 });
  const row = (id: string, offset: number): NormalizedChatMessage => ({ id, type: "assistant", content: "opaque",
    sourceSessionId: "own", sourceRecord: record(offset), timestamp: String(send.sentAtMs + offset) });
  const sourceRows = [row("answer", 10), row("later-output", 30)];
  const transcript = (rows: NormalizedChatMessage[]) => reduceTranscript(createTranscriptState(), {
    kind: "snapshot", authority, range: null, rows,
  }).state;
  const state = memory();
  const delivered: DeliverySend = { ...send, matchedRow: { source: { source_session_id: "own", record: record(20) }, ordinal: 1 },
    status: { tab_id: "tab", send_id: "tx", correlationId: "token", state: "delivered", sequence: 2,
      ledger_id: "ledger", retained_from: 1, error: null } };
  const render = (current: DeliverySend, rows: NormalizedChatMessage[]) => {
    const sends = deliveryProjectionSends([current], null, queuedDeliveryIds([current], true, state));
    const projection = projectTranscript(transcript(rows), sends);
    return stableDeliveryEntries(projection.rows.map(entry), sends, projection.echoes, state).map(row => row.msg.id);
  };
  const phases = [render(send, sourceRows), render(delivered, sourceRows), render(delivered, sourceRows),
    render(delivered, [sourceRows[0], { ...row("provider-row", 20), type: "user", content: "enriched caption" }, sourceRows[1]])];
  assert.deepEqual(phases, [
    ["answer", "later-output", "local"], ["answer", "local", "later-output"],
    ["answer", "local", "later-output"], ["answer", "local", "later-output"],
  ]);
  const positions = phases.map(rows => rows.indexOf(send.id));
  assert.equal(positions.filter((position, index) => index > 0 && position !== positions[index - 1]).length, 1);
});

test("a ledger-confirmed merged record cannot resurrect a retained second bubble", () => {
  const state = memory();
  const sends = [send, { ...send, id: "second", correlationId: "second", content: "second" }];
  stableDeliveryEntries(sends.map(send => entry({ id: send.id, type: "user", content: send.content })), sends, new Map(), state);
  const confirmed = sends.map((send, index) => ({ ...send, matchedRow: { ordinal: index + 1,
    source: { source_session_id: "own", record: { kind: "jsonl" as const, incarnation: "file", record_start: 10, record_row: 0 } } } }));
  const source = entry({ id: "source", sourceSessionId: "own", sourceRecord: confirmed[0].matchedRow.source.record,
    type: "user", content: "opaque merged source" });
  const echoes = new Map(confirmed.map(send => [send.id, { content: source.msg.content, timestampMs: null }]));
  const rows = stableDeliveryEntries([source], confirmed, echoes, state);
  assert.equal(rows.length, 1);
  assert.equal(state.pending.size, 0);
});

test("a delivered claim whose record cannot be located stays one tail bubble until its row takes it over", () => {
  const authority = { sourceSessionId: "own", sessionBindingNonce: "launch" };
  const record = { kind: "jsonl" as const, incarnation: "file", record_start: 40, record_row: 0 };
  const output: NormalizedChatMessage = { id: "output", type: "assistant", content: "streaming", sourceSessionId: "own",
    timestamp: String(send.sentAtMs + 10) };
  const delivered: DeliverySend = { ...send, matchedRow: { source: { source_session_id: "own", record }, ordinal: 1 },
    status: { tab_id: "tab", send_id: "tx", correlationId: "token", state: "delivered", sequence: 2, ledger_id: "ledger", retained_from: 1, error: null } };
  for (const sourceRecord of [undefined, { ...record, incarnation: "" }, { ...record, incarnation: "regrown" }]) {
    const state = memory();
    const render = (current: DeliverySend, rows: NormalizedChatMessage[]) => {
      const sends = deliveryProjectionSends([current], null, queuedDeliveryIds([current], true, state));
      const projection = projectTranscript(reduceTranscript(createTranscriptState(), { kind: "snapshot", authority, range: null, rows }).state, sends);
      return stableDeliveryEntries(projection.rows.map(entry), sends, projection.echoes, state).map(row => row.msg.id);
    };
    const echo: NormalizedChatMessage = { id: "provider-row", type: "user", content: send.content, sourceSessionId: "own",
      timestamp: String(send.sentAtMs + 20), ...(sourceRecord ? { sourceRecord } : {}) };
    assert.deepEqual([render(send, [output]), render(delivered, [output]), render(delivered, [output, echo])],
      [["output", "local"], ["output", "local"], ["output", "local"]], `record ${JSON.stringify(sourceRecord)}`);
  }
});

test("rows flattened from one claimed record keep their keys; only the first takes the send identity", () => {
  const record = { kind: "jsonl" as const, incarnation: "file", record_start: 20, record_row: 0 };
  const claimed: DeliverySend = { ...send, matchedRow: { source: { source_session_id: "own", record }, ordinal: 1 } };
  const rows = ["first", "second"].map(id => entry({ id, type: "user", content: id, sourceSessionId: "own", sourceRecord: record }));
  const echoes = new Map([[send.id, { content: "first", timestampMs: null }]]);
  assert.deepEqual(stableDeliveryEntries(rows, [claimed], echoes, memory()).map(row => row.msg.id), [send.id, "second"]);
});
