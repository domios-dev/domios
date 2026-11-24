import assert from "node:assert/strict";
import { test } from "node:test";
import { createTranscriptState, projectTranscript, projectTranscriptParts, reduceTranscript, type MessageSegment, type NormalizedChatMessage } from "@codeterm/chat-core";
import { applyDeliveryStatus, deliveryProjectionSends, type DeliverySend } from "./chatDeliveryModel";
import { createDeliveryProjectionMemory, stableDeliveryEntries } from "./chatDeliveryProjection";
import { enrichTimelineItems } from "./chatEnrichment";
import { conversationEntries } from "./chatPagingModel";

const body = "opaque routed payload";
const wire = `<domios from="tab" label="Sender" tab="a1b2c3d4">${body} <tip>opaque reply hint</tip></domios>`;
const segments: MessageSegment[] = [{ type: "frame", kind: "tab", from: "tab", attrs: { from: "tab", label: "Sender", tab: "a1b2c3d4" }, body, tip: "opaque reply hint" }];
const render = (rows: NormalizedChatMessage[]) => enrichTimelineItems(rows.map((message) => ({ kind: "message", message })), new WeakMap());

for (const state of ["held", "written", "submit_unconfirmed", "delivered"] as const) {
  test(`trusted routed ${state} send is clean on its first projection`, () => {
    const send: DeliverySend = {
      id: "send", correlationId: "send", content: wire, sentAtMs: 1000,
      status: { tab_id: "tab", send_id: "send", correlationId: "send", state,
        ledger_id: "ledger", sequence: 1, retained_from: 1, error: null,
        system_send: { content: wire, sent_at_ms: 1000 } },
    };
    const before = structuredClone(send);
    const rows = projectTranscript(createTranscriptState(), deliveryProjectionSends([send])).rows;
    const initial = render(rows);
    const committed = render([{ id: "echo", type: "user", content: wire, segments }]);
    assert.deepEqual(initial.map((entry) => [entry.msg.content, entry.origin]), committed.map((entry) => [entry.msg.content, entry.origin]));
    assert.equal(initial[0].msg.content, body);
    assert.deepEqual(send, before);
  });
}

test("untrusted user text keeps its literal leading envelope", () => {
  const send: DeliverySend = { id: "literal", content: wire, sentAtMs: 1000 };
  const [entry] = render(projectTranscript(createTranscriptState(), deliveryProjectionSends([send])).rows);
  assert.equal(entry.msg.content, wire);
  assert.equal(entry.origin, undefined);
});

test("a structured record claim keeps the routed card and key through plain echoes and tool updates", () => {
  const authority = { sourceSessionId: "session", sessionBindingNonce: "launch" };
  const at = 1_800_000_000_000;
  const pending: DeliverySend = {
    id: "send", correlationId: "route-token", content: wire, sentAtMs: at,
    status: { tab_id: "target", send_id: "send", correlationId: "route-token", state: "written",
      ledger_id: "ledger", sequence: 1, retained_from: 1, error: null,
      system_send: { content: wire, sent_at_ms: at, segments } },
  };
  const delivered = applyDeliveryStatus(pending, {
    ...pending.status!, state: "delivered", sequence: 2,
    matched_row: { source: { source_session_id: "session", record: {
      kind: "structured", thread_id: "session", item_id: "provider-row",
    } }, ordinal: 1 },
  });
  const before: NormalizedChatMessage = { id: "before", type: "assistant", content: "before", timestamp: String(at - 1000) };
  const after: NormalizedChatMessage = { id: "after", type: "tool_call", content: "after", timestamp: String(at + 1000) };
  const echo: NormalizedChatMessage = { id: "provider-row", type: "user", content: wire, timestamp: String(at),
    segments: [{ type: "text", text: wire }] };
  const memory = createDeliveryProjectionMemory("session");
  let transcript = createTranscriptState();
  const project = (rows: NormalizedChatMessage[], send: DeliverySend, kind: "snapshot" | "delta" = "delta") => {
    transcript = reduceTranscript(transcript, { kind, authority, range: null, rows }).state;
    const sends = deliveryProjectionSends([send]);
    const result = projectTranscript(transcript, sends);
    return stableDeliveryEntries(conversationEntries(result.rows), sends, result.echoes, memory, result.outside);
  };

  const first = project([before, after], pending, "snapshot");
  assert.deepEqual(first.map(entry => entry.msg.id), ["before", "send", "after"]);
  assert.equal(first[1].msg.content, body);
  assert.equal(first[1].origin?.kind, "tab");
  for (const rows of [
    [echo],
    [{ ...echo, content: body, inboundKind: "tab" as const, segments }],
    [{ ...echo, content: `<pasted_content>${wire}</pasted_content>`, segments: [{ type: "paste" as const, id: null, body: wire }] },
      { ...after, toolStatus: "completed" as const }],
  ]) {
    const projected = project(rows, delivered);
    assert.deepEqual(projected.map(entry => entry.msg.id), ["before", "send", "after"]);
    assert.equal(projected[1].msg.content, body);
    assert.deepEqual(projected[1].origin, first[1].origin);
    assert.equal(projected[1].msg.routing?.tip, "opaque reply hint");
  }
  const unrelated = project([{ ...echo, id: "other-row" }], delivered).find(entry => entry.msg.sourceMessageId === "other-row");
  assert.equal(unrelated?.msg.content, wire);
  assert.equal(unrelated?.origin, undefined);
});

test("own-session rows with the host send ID keep one routed key before the claim arrives", () => {
  const authority = { sourceSessionId: "session", sessionBindingNonce: "launch" };
  const send: DeliverySend = {
    id: "host-id", correlationId: "route-token", content: wire, sentAtMs: 2000,
    status: { tab_id: "target", send_id: "host-id", correlationId: "route-token", state: "written",
      ledger_id: "ledger", sequence: 1, retained_from: 1, error: null,
      system_send: { content: wire, sent_at_ms: 2000, segments } },
  };
  const sends = deliveryProjectionSends([send]);
  const memory = createDeliveryProjectionMemory("session");
  const base: NormalizedChatMessage = { id: send.id, type: "user", content: wire };
  for (const row of [
    { ...base, segments: [{ type: "text" as const, text: wire }] },
    { ...base, content: body, inboundKind: "tab" as const, segments },
  ]) {
    const transcript = reduceTranscript(createTranscriptState(), { kind: "snapshot", authority, range: null, rows: [row] }).state;
    const result = projectTranscript(transcript, sends);
    const entries = stableDeliveryEntries(render(result.rows), sends, result.echoes, memory);
    assert.deepEqual(entries.map(({ msg, origin }) => [msg.id, msg.content, origin?.kind]), [[send.id, body, "tab"]]);
  }
});

for (const kind of ["tab", "report", "mesh", "system", "telegram", "curator", "cli"] as const) {
  test(`${kind} host segments survive a correlated plain provider echo`, () => {
    const attrs = { from: kind, label: "Weaver", tab: "8494e5c7", model: "opaque-model", user: "123", peer: "peer", context: "handoff" };
    const wire = `<domios from="${kind}" label="Weaver" tab="8494e5c7" model="opaque-model" user="123" peer="peer" context="handoff">**opaque body**<tip>opaque reply command</tip></domios>`;
    const segments: MessageSegment[] = [kind === "curator" || kind === "cli"
      ? { type: "unknown_frame", raw: wire, attrs, body: "**opaque body**<tip>opaque reply command</tip>" }
      : { type: "frame", kind, from: kind, attrs, body: "**opaque body**", tip: "opaque reply command" }];
    const send: DeliverySend = { id: "host-send", correlationId: "host-token", content: wire, sentAtMs: 1000,
      status: { tab_id: "target", send_id: "host-send", correlationId: "host-token", state: "written", ledger_id: "ledger",
        sequence: 1, retained_from: 1, error: null, system_send: { content: wire, sent_at_ms: 1000, segments, segments_version: 2 } } };
    const projected = deliveryProjectionSends([send]);
    for (const type of ["text", "paste"] as const) {
      const echo: NormalizedChatMessage = { id: "provider-echo", type: "user", content: wire, correlationId: "host-token",
        segments: [type === "text" ? { type, text: wire } : { type, id: null, body: wire }] };
      const [entry] = render(projectTranscriptParts([echo], projected));
      assert.equal(entry.msg.content, "**opaque body**");
      assert.equal(entry.msg.routing?.kind, kind);
      assert.equal(entry.msg.routing?.tip, "opaque reply command");
      assert.ok(entry.origin);
      const [foreign] = render(projectTranscriptParts([{ ...echo, correlationId: "foreign" }], projected));
      assert.equal(foreign.origin, undefined);
      assert.equal(foreign.msg.content, wire);
      const [authored] = render(projectTranscriptParts([{ ...echo, type: "assistant" }], projected));
      assert.equal(authored.origin, undefined);
      assert.equal(authored.msg.content, wire);
    }
    const [quoted] = render(projectTranscriptParts([{ id: "quoted", type: "user", correlationId: "host-token", content: `Quoted example: ${wire}` }], projected));
    assert.equal(quoted.origin, undefined);
    assert.equal(quoted.msg.routing, undefined);
  });
}

test("same-sequence host-segment enrichment upgrades routed sends while untrusted and mismatched echoes stay plain", () => {
  const dumasBody = "opaque dumas instruction";
  const dumasWire = `<domios from="tab" label="Dumas" tab="duma0001">${dumasBody} <tip>reply to dumas</tip></domios>`;
  const dumasSegments: MessageSegment[] = [{
    type: "frame", kind: "tab", from: "tab",
    attrs: { from: "tab", label: "Dumas", tab: "duma0001" }, body: dumasBody, tip: "reply to dumas",
  }];
  const base: DeliverySend = { id: "dumas", correlationId: "dumas-token", content: dumasWire, sentAtMs: 1000 };
  const initial = applyDeliveryStatus(base, {
    tab_id: "tab", send_id: "dumas", correlationId: "dumas-token", state: "written",
    ledger_id: "ledger", sequence: 1, retained_from: 1, error: null,
  });
  const [beforeEnrichment] = render(projectTranscript(createTranscriptState(), deliveryProjectionSends([initial])).rows);
  assert.equal(beforeEnrichment.origin, undefined);
  assert.equal(beforeEnrichment.msg.content, dumasWire);

  const enriched = applyDeliveryStatus(initial, {
    tab_id: "tab", send_id: "dumas", correlationId: "dumas-token", state: "written",
    ledger_id: "ledger", sequence: 1, retained_from: 1, error: null,
    system_send: { content: dumasWire, sent_at_ms: 1000, segments: dumasSegments, segments_version: 2 },
  });
  const projected = deliveryProjectionSends([enriched]);
  const pending = render(projectTranscript(createTranscriptState(), projected).rows);
  const plainEcho: NormalizedChatMessage = {
    id: "provider-echo", type: "user", content: dumasWire, correlationId: "dumas-token",
    segments: [{ type: "text", text: dumasWire }],
  };
  const echoed = render(projectTranscriptParts([plainEcho], projected));
  assert.deepEqual(pending.map((entry) => [entry.msg.content, entry.origin]), echoed.map((entry) => [entry.msg.content, entry.origin]));
  assert.equal(echoed[0].msg.content, dumasBody);
  assert.equal(echoed[0].origin?.kind, "tab");
  assert.equal(echoed[0].origin?.kind === "tab" ? echoed[0].origin.title : undefined, "Dumas");
  assert.equal(echoed[0].msg.routing?.tip, "reply to dumas");

  const [wrongToken] = render(projectTranscriptParts([{ ...plainEcho, correlationId: "wrong-token" }], projected));
  assert.equal(wrongToken.origin, undefined);
  assert.equal(wrongToken.msg.content, dumasWire);
  const [assistantRow] = render(projectTranscriptParts([{ ...plainEcho, type: "assistant" }], projected));
  assert.equal(assistantRow.origin, undefined);
  assert.equal(assistantRow.msg.content, dumasWire);
  const [quoted] = render(projectTranscriptParts([{ ...plainEcho, content: `Quoted: ${dumasWire}`, segments: [{ type: "text", text: `Quoted: ${dumasWire}` }] }], projected));
  assert.equal(quoted.origin, undefined);
  assert.equal(quoted.msg.routing, undefined);
});
