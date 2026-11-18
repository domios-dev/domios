import assert from "node:assert/strict";
import { test } from "node:test";
import { createTranscriptState, projectTranscript, projectTranscriptParts, type MessageSegment, type NormalizedChatMessage } from "@codeterm/chat-core";
import { deliveryProjectionSends, type DeliverySend } from "./chatDeliveryModel";
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

