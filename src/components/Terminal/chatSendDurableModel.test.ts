import assert from "node:assert/strict";
import { test } from "node:test";
import { decodeChatSend, durableRecordByteLength, encodeChatSend, mergeChatSendRecord,
  orderChatSendRecords, sameDurableSend } from "./chatSendDurableModel";
import type { DeliverySend } from "./chatDeliveryModel";

test("durable sends round-trip authority, recovery fields, and only exact duplicate bodies once", () => {
  const matchedRow = {
    source: { source_session_id: "source-session", record: { kind: "structured" as const, thread_id: "source-session", item_id: "item-1" } },
    ordinal: 3,
  };
  const send: DeliverySend = {
    id: "send-1",
    correlationId: "correlation-1",
    content: "payload",
    sentAtMs: 101,
    echo: { content: "payload", timestampMs: 102 },
    matchedRow,
    echoBoundary: { sourceSessionId: "source-session", sessionBindingNonce: "nonce-1", lastRowId: "prior-row" },
    droppedUploads: ["private.bin"],
    dismissed: true,
    status: {
      send_id: "backend-send-1", tab_id: "ignored-runtime-tab", state: "delivered", error: null,
      ledger_id: "ledger-1", sequence: 9, retained_from: 2, correlationId: "correlation-1",
      matched_row: matchedRow, system_send: { content: "payload", sent_at_ms: 101 },
    },
    requestFailed: false,
    requestUncertain: false,
    requestHadResponse: true,
    draft: { text: "payload", attachments: ["/draft/file"], recordedVoiceAttachments: ["/voice/file"], capsules: {}, reply: null },
    echoEligible: true,
    flowAtMs: 101,
    segments: [{ type: "text", text: "payload" }],
  };

  const record = encodeChatSend("tab-1", send);
  const restored = decodeChatSend(record);

  assert.deepEqual(record.bodies, ["payload"]);
  assert.equal(record.send?.matchedRow, undefined);
  assert.equal(record.send?.matchedRowInStatus, true);
  assert.equal(JSON.stringify(record).includes("tab_id"), false);
  assert.deepEqual(restored, {
    id: send.id,
    content: send.content,
    sentAtMs: send.sentAtMs,
    correlationId: send.correlationId,
    echo: send.echo,
    matchedRow,
    echoBoundary: send.echoBoundary,
    droppedUploads: send.droppedUploads,
    dismissed: true,
    status: { ...send.status, tab_id: "tab-1" },
    requestFailed: false,
    requestUncertain: false,
    requestHadResponse: true,
    draft: send.draft,
  });
});

test("durable equality includes referenced body content", () => {
  const original: DeliverySend = { id: "send", content: "first", sentAtMs: 1 };
  assert.equal(sameDurableSend(original, { ...original }), true);
  assert.equal(sameDurableSend(original, { ...original, content: "second" }), false);
});

test("stale writes preserve delivered row claims and dismissal tombstones", () => {
  const matchedRow = {
    source: { source_session_id: "source", record: { kind: "structured" as const, thread_id: "source", item_id: "row" } },
    ordinal: 1,
  };
  const previous: DeliverySend = { id: "send", correlationId: "token", content: "original", sentAtMs: 1 };
  const delivered: DeliverySend = {
    ...previous,
    echo: { content: "recorded", timestampMs: 2 },
    matchedRow,
    dismissed: true,
    status: { send_id: "tx", tab_id: "tab", state: "delivered", error: null, ledger_id: "ledger", sequence: 7,
      retained_from: 1, correlationId: "token", matched_row: matchedRow },
  };
  const stale: DeliverySend = {
    ...previous,
    status: { send_id: "tx", tab_id: "tab", state: "written", error: null, ledger_id: "ledger", sequence: 3,
      retained_from: 1, correlationId: "token" },
    dismissed: false,
  };

  const merged = mergeChatSendRecord(delivered, previous, stale);

  assert.equal(merged.status?.state, "delivered");
  assert.deepEqual(merged.matchedRow, matchedRow);
  assert.deepEqual(merged.echo, delivered.echo);
  assert.equal(merged.dismissed, true);
});

test("tab records restore in their persisted append order", () => {
  const later = encodeChatSend("tab", { id: "later", content: "later", sentAtMs: 2 }, 0, 7);
  const earlier = encodeChatSend("tab", { id: "earlier", content: "earlier", sentAtMs: 1 }, 0, 4);
  assert.deepEqual(orderChatSendRecords([later, earlier]).map((record) => record.sendId), ["earlier", "later"]);
});

test("synthetic multi-tab records serialize a changed send independently", (t) => {
  const payload = "x".repeat(512);
  const sends = Array.from({ length: 1024 }, (_, index): DeliverySend => ({
    id: `send-${index}`,
    correlationId: `token-${index}`,
    content: payload,
    sentAtMs: index,
    echo: { content: payload, timestampMs: index },
    status: {
      send_id: `tx-${index}`, tab_id: `tab-${Math.floor(index / 32)}`, state: "written", error: null,
      ledger_id: "ledger", sequence: 1, retained_from: 1, correlationId: `token-${index}`,
      system_send: { content: payload, sent_at_ms: index },
    },
    draft: { text: payload, attachments: [], recordedVoiceAttachments: [], capsules: {}, reply: null },
  }));
  const tabSends = sends.map((send, index) => encodeChatSend(`tab-${Math.floor(index / 32)}`, send));
  const globalCollectionBytes = new TextEncoder().encode(JSON.stringify(Object.fromEntries(
    Array.from({ length: 32 }, (_, tabIndex) => [`tab-${tabIndex}`, sends.slice(tabIndex * 32, (tabIndex + 1) * 32)]),
  ))).byteLength;
  const changed = encodeChatSend("tab-0", { ...sends[0], dismissed: true }, 1);

  assert.equal(tabSends.length, 1024);
  assert.equal(tabSends[0].bodies.length, 1);
  assert.equal(changed.bodies.length, 1);
  assert(durableRecordByteLength(changed) < globalCollectionBytes);
  t.diagnostic(`synthetic records: global collection bytes=${globalCollectionBytes}; one changed record bytes=${durableRecordByteLength(changed)}`);
});
