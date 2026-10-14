import "./ChatComposer.composition.harness";
import assert from "node:assert/strict";
import { test } from "node:test";
import { canDeleteChatSend, visibleDeliveryRows, type DeliverySend } from "./chatDeliveryModel";
import { addChatSend, observeChatSendDismissal, observeDeliveryStatus, forgetChatSendsInMemory, readChatSends, beginChatSendRetry } from "./chatSendStore";
import type { ChatSendStatus } from "./chatSendStatus";

const status: ChatSendStatus = { send_id: "tx", tab_id: "delete", correlationId: "token", state: "held", error: null, ledger_id: "ledger", sequence: 1, retained_from: 1 };
const send: DeliverySend = { id: "bubble", correlationId: "token", content: "opaque", sentAtMs: 42, status };

test("Delete is available only for retained undelivered user sends", () => {
  for (const state of ["held", "written", "submit_unconfirmed", "failed"] as const) assert.equal(canDeleteChatSend({ ...send, status: { ...status, state } }), true);
  assert.equal(canDeleteChatSend({ ...send, status: { ...status, state: "delivered" } }), false);
  assert.equal(canDeleteChatSend({ ...send, echo: { content: "opaque", timestampMs: 42 } }), false);
  assert.equal(canDeleteChatSend({ ...send, status: undefined }), false);
  assert.equal(canDeleteChatSend({ ...send, status: undefined, requestFailed: true }), true);
  assert.equal(canDeleteChatSend({ ...send, status: undefined, requestUncertain: true }), true);
  assert.equal(canDeleteChatSend({ ...send, status: { ...status, system_send: { content: "opaque", sent_at_ms: 42 } } }), false);
});

test("dismissal survives reload and late evidence without offering Retry or restoring the row", () => {
  addChatSend("delete", send);
  observeChatSendDismissal("delete", { send_id: "tx", send });
  forgetChatSendsInMemory();
  observeDeliveryStatus({ ...status, state: "delivered" });
  const restored = readChatSends("delete");
  assert.equal(restored[0].dismissed, true);
  assert.equal(beginChatSendRetry("delete", "bubble"), undefined);
  assert.deepEqual(visibleDeliveryRows([
    { id: "bubble", type: "user", content: "opaque" },
    { id: "other", type: "assistant", content: "answer" },
  ], restored).map((row) => row.id), ["other"]);
});

test("durable receipts restore a hidden correlation when the local cache is absent", () => {
  observeChatSendDismissal("empty-delete", { send_id: "tx", send });
  const restored = readChatSends("empty-delete");
  assert.equal(restored[0].dismissed, true);
  assert.deepEqual(visibleDeliveryRows([{ id: "echo", correlationId: "token", type: "user", content: "opaque" }], restored), []);
});

test("a late transcript echo is consumed by the hidden send across projection", async () => {
  const { createTranscriptState, reduceTranscript, projectTranscript } = await import("@codeterm/chat-core");
  const { deliveryProjectionSends } = await import("./chatDeliveryModel");
  const hidden: DeliverySend = { ...send, dismissed: true, sentAtMs: 1700000000000,
    status: { ...status, state: "submit_unconfirmed" } };
  const transcript = reduceTranscript(createTranscriptState(), {
    kind: "snapshot", authority: { sourceSessionId: "session", sessionBindingNonce: "nonce" },
    range: { start: 0, end: 100 }, rows: [
      { id: "source-echo", type: "user", content: "opaque", timestamp: "1700000000001" },
      { id: "reply", type: "assistant", content: "answer", timestamp: "1700000000002" },
    ],
  }).state;
  const projected = projectTranscript(transcript, deliveryProjectionSends([hidden]));
  assert.equal(projected.echoes.has("bubble"), true);
  assert.deepEqual(visibleDeliveryRows(projected.rows, [hidden], projected.echoes).map((row) => row.id), ["reply"]);
});
