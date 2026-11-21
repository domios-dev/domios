import assert from "node:assert/strict";
import { test } from "node:test";
import { createTranscriptState, projectTranscript, projectTranscriptParts, type MessageSegment, type NormalizedChatMessage } from "@codeterm/chat-core";
import { applyDeliveryStatus, deliveryProjectionSends, type DeliverySend } from "./chatDeliveryModel";
import { enrichTimelineItems } from "./chatEnrichment";

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

for (const kind of ["tab", "report", "mesh", "system", "telegram", "curator"] as const) {
  test(`${kind} host segments survive a correlated plain provider echo`, () => {
    const attrs = { from: kind, label: "Weaver", tab: "8494e5c7", model: "opaque-model", user: "123", peer: "peer", context: "handoff" };
    const wire = `<domios from="${kind}" label="Weaver" tab="8494e5c7" model="opaque-model" user="123" peer="peer" context="handoff">**opaque body**<tip>opaque reply command</tip></domios>`;
    const segments: MessageSegment[] = [kind === "curator"
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
