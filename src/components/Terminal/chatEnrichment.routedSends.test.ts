import assert from "node:assert/strict";
import { test } from "node:test";
import { createTranscriptState, projectTranscript, type MessageSegment, type NormalizedChatMessage } from "@codeterm/chat-core";
import { deliveryProjectionSends, type DeliverySend } from "./chatDeliveryModel";
import { enrichTimelineItems } from "./chatEnrichment";

const body = "opaque routed payload";
const wire = `<domios from="tab" label="Sender" tab="a1b2c3d4">${body} <tip>opaque reply hint</tip></domios>`;
const segments: MessageSegment[] = [{ type: "frame", from: "tab", attrs: { from: "tab", label: "Sender", tab: "a1b2c3d4" }, body, tip: "opaque reply hint" }];
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

