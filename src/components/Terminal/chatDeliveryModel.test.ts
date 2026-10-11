import assert from "node:assert/strict";
import { test } from "node:test";
import { applyDeliveryStatus, deliveryState, sendForRow, type DeliverySend } from "./chatDeliveryModel";
import type { ChatSendStatus } from "./chatSendStatus";

const send = (): DeliverySend => ({ id: "bubble", correlationId: "token", content: "opaque", sentAtMs: 1000 });
const status = (state: ChatSendStatus["state"], sequence: number): ChatSendStatus => ({
  tab_id: "tab", send_id: "transaction", state, sequence, ledger_id: "ledger",
  retained_from: 1, error: null, correlationId: "token",
});

test("sending becomes delivered without changing the bubble", () => {
  const initial = send();
  assert.equal(deliveryState(initial), "sending");
  const written = applyDeliveryStatus(initial, status("written", 2));
  assert.equal(deliveryState(written), "sending");
  const delivered = applyDeliveryStatus(written, status("delivered", 3));
  assert.equal(deliveryState(delivered), "delivered");
  assert.equal(delivered.id, "bubble");
});

test("held queues and exact evidence delivers the same bubble", () => {
  const queued = applyDeliveryStatus(send(), status("held", 1));
  assert.equal(deliveryState(queued), "queued");
  const delivered = applyDeliveryStatus(queued, status("delivered", 4));
  assert.equal(deliveryState(delivered), "delivered");
  assert.equal(delivered.id, queued.id);
});

test("uncertain writes show uncertainty and stale responses cannot downgrade evidence", () => {
  const uncertain = applyDeliveryStatus(send(), status("submit_unconfirmed", 3));
  assert.equal(deliveryState(uncertain), "uncertain");
  assert.equal(applyDeliveryStatus(uncertain, status("held", 1)), uncertain);
  const delivered = applyDeliveryStatus(uncertain, status("delivered", 4));
  assert.equal(applyDeliveryStatus(delivered, status("failed", 5)), delivered);
});

test("Retry's fresh transaction ignores the old failed token", () => {
  const failed = applyDeliveryStatus(send(), status("failed", 4));
  assert.equal(deliveryState(failed), "failed");
  const retry = { ...failed, status: undefined, correlationId: "retry-token" };
  assert.equal(deliveryState(retry), "sending");
  assert.equal(applyDeliveryStatus(retry, status("failed", 5)), retry);
  const delivered = applyDeliveryStatus(retry, { ...status("delivered", 6), send_id: "retry-transaction", correlationId: "retry-token" });
  assert.equal(deliveryState(delivered), "delivered");
  assert.equal(delivered.id, "bubble");
});

test("source echo resolves to the optimistic send by correlation or recorded identity", () => {
  const row = { id: "source", type: "user" as const, content: "opaque", timestamp: "1000", correlationId: "token" };
  const initial = send();
  assert.equal(sendForRow([initial], row), initial);
  const recorded = { ...initial, echo: { content: "opaque", timestampMs: 1000000 } };
  assert.equal(sendForRow([recorded], { ...row, correlationId: undefined }), recorded);
  assert.equal(sendForRow([recorded], { ...row, type: "assistant" }), undefined);
});
