import assert from "node:assert/strict";
import { test } from "node:test";
import { createTranscriptState, reduceTranscript, projectTranscript, type NormalizedChatMessage } from "@codeterm/chat-core";
import { deliveryProjectionSends, queuedBehindTurn, type DeliverySend } from "./chatDeliveryModel";
import { createDeliveryProjectionMemory, queuedDeliveryIds, stableDeliveryEntries } from "./chatDeliveryProjection";
import type { EnrichedChatEntry } from "./chatEnrichment";
import { conversationEntries } from "./chatPagingModel";

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

test("a transcribing voice bubble stays newest despite newer server timestamps", () => {
  const voice: DeliverySend = { ...send, sentAtMs: 10,
    content: '<domios type="attachment" kind="voice" path="/recording.webm" label="voice" />',
    draft: { text: "", attachments: ["/recording.webm"], recordedVoiceAttachments: ["/recording.webm"],
      voiceTranscribing: true, capsules: {}, reply: null } };
  const rows: NormalizedChatMessage[] = [{ id: "later-output", type: "assistant", content: "still working", timestamp: "200" }];
  const projected = deliveryProjectionSends([voice]);
  assert.equal(projected[0]?.held, true);
  assert.deepEqual(projectTranscript(reduceTranscript(createTranscriptState(), { kind: "snapshot",
    authority: { sourceSessionId: "own", sessionBindingNonce: "launch" }, range: null, rows }).state, projected)
    .rows.map(row => row.id), ["later-output", "local"]);
  assert.match(projected[0]!.content, /kind="voice" path="\/recording\.webm"/);
});

test("the transcript echo takes over the pending voice bubble identity", () => {
  const voice: DeliverySend = { ...send, content: "transcribing voice", draft: {
    text: "", attachments: ["/recording.webm"], recordedVoiceAttachments: ["/recording.webm"],
    voiceTranscribing: true, capsules: {}, reply: null,
  } };
  const state = memory();
  const pending = entry({ id: voice.id, type: "user", content: voice.content, timestamp: String(voice.sentAtMs) });
  assert.equal(stableDeliveryEntries([pending], [voice], new Map(), state)[0]?.msg.id, voice.id);
  const transcript = entry({ id: "server-row", sourceMessageId: "server-row", sourceSessionId: "own", type: "user",
    correlationId: voice.correlationId, content: "spoken words", timestamp: String(voice.sentAtMs + 100) });
  const rows = stableDeliveryEntries([transcript], [{ ...voice, content: "spoken words", echo: {
    content: "spoken words", timestampMs: voice.sentAtMs + 100,
  } }], new Map(), state);
  assert.equal(rows.length, 1);
  assert.equal(rows[0]?.msg.id, voice.id);
  assert.equal(rows[0]?.msg.content, "spoken words");
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

test("mid-turn routed deliveries take their own slots before a later owner exchange", () => {
  const thread = "thread";
  const authority = { sourceSessionId: thread, sessionBindingNonce: "launch" };
  const minute = 60_000;
  const row = (id: string, offset: number, type: NormalizedChatMessage["type"] = "assistant"): NormalizedChatMessage => ({
    id, type, content: id, timestamp: String(send.sentAtMs + offset * minute),
  });
  const source = [row("start", 0, "user"), row("work-1", 10), row("work-2", 20), row("work-3", 38),
    row("owner", 40, "user"), row("reply", 41)];
  const snapshot = reduceTranscript(createTranscriptState(), {
    kind: "snapshot", authority, range: { start: 0, end: 100 }, rows: source,
  }).state;
  const routed = [5, 15, 25].map((offset, index): DeliverySend => {
    const id = `send-${index}`;
    const content = `<domios from="tab" label="Sender" tab="source">${id}</domios>`;
    const sentAtMs = send.sentAtMs + offset * minute;
    return { id, correlationId: id, content, sentAtMs, status: {
      tab_id: "tab", send_id: id, correlationId: id, state: "held", sequence: 1, ledger_id: "ledger",
      retained_from: 1, error: null, pending_reason: "held_active_turn", system_send: { content, sent_at_ms: sentAtMs },
    } };
  });
  const delivered = routed.map((current, index): DeliverySend => ({
    ...current,
    matchedRow: { source: { source_session_id: thread, record: {
      kind: "structured", thread_id: thread, item_id: `provider-${index}`,
    } }, ordinal: 1 },
    status: { ...current.status!, state: "delivered", sequence: 2, pending_reason: undefined },
  }));
  const state = memory();
  const render = (sends: DeliverySend[], transcript = snapshot) => {
    const projectedSends = deliveryProjectionSends(sends, null, queuedDeliveryIds(sends, true, state));
    const projection = projectTranscript(transcript, projectedSends);
    const rendered = stableDeliveryEntries(conversationEntries(projection.rows), projectedSends, projection.echoes, state, projection.outside);
    return { projection, rows: rendered.map(({ msg }) => msg) };
  };
  assert.deepEqual(render(routed).rows.slice(-3).map(item => item.id), ["send-0", "send-1", "send-2"]);
  const receipts = render([delivered[0], delivered[1], routed[2]]);
  assert.deepEqual(receipts.rows.map(item => item.id),
    ["start", "send-0", "work-1", "send-1", "work-2", "work-3", "owner", "reply", "send-2"]);

  const provider = deliveryProjectionSends(delivered).map((current, index) => ({
    ...row(`provider-${index}`, [12, 17, 30][index], "user"), content: current.content, segments: current.segments,
  }));
  const transcript = reduceTranscript(snapshot, { kind: "delta", authority, range: null, rows: [
    provider[2], { ...provider[0], id: routed[0].id, runtimeSourceItemId: provider[0].id, timestamp: undefined },
    provider[0], provider[1],
  ] }).state;
  const committed = render(delivered, transcript);
  assert.deepEqual(committed.projection.pending, []);
  assert.equal(committed.projection.echoes.size, 3);
  assert.deepEqual(committed.rows.map(item => item.id),
    ["start", "work-1", "send-0", "send-1", "work-2", "send-2", "work-3", "owner", "reply"]);
  assert.deepEqual(committed.rows.filter(item => item.routing?.kind === "tab").map(item => item.timestamp),
    provider.map(item => item.timestamp));
});
