import "./ChatComposer.composition.harness";
import assert from "node:assert/strict";
import { test } from "node:test";
import {
  addChatSend, beginChatSendRetry, failChatSendRequest, forgetChatSendsInMemory,
  observeDeliveryStatus, readChatSends, reconcileDeliveryStatuses, setChatSendDraft,
} from "./chatSendStore";
import { deliveryState } from "./chatDeliveryModel";
import type { ChatSendStatus } from "./chatSendStatus";

test("Retry preserves the row, guards repeated clicks, and accepts only the fresh transaction", () => {
  const tabId = "retry-store";
  addChatSend(tabId, { id: "bubble", correlationId: "original-token", content: "opaque", sentAtMs: 1700000000000 });
  const status: ChatSendStatus = { tab_id: tabId, send_id: "first", state: "failed", ledger_id: "ledger",
    sequence: 4, retained_from: 1, error: null, correlationId: "original-token" };
  observeDeliveryStatus(status);
  const retry = beginChatSendRetry(tabId, "bubble");
  assert(retry);
  assert.notEqual(retry.correlationId, "original-token");
  assert.equal(retry.id, "bubble");
  assert.equal(retry.sentAtMs, 1700000000000);
  assert.equal(beginChatSendRetry(tabId, "bubble"), undefined);
  observeDeliveryStatus({ ...status, sequence: 5 });
  assert.equal(deliveryState(readChatSends(tabId)[0]), "sending");
  observeDeliveryStatus({ ...status, send_id: "second", sequence: 6, state: "delivered", correlationId: retry.correlationId });
  assert.equal(readChatSends(tabId).length, 1);
  assert.equal(deliveryState(readChatSends(tabId)[0]), "delivered");
  assert.equal(beginChatSendRetry(tabId, "bubble"), undefined);
});

test("unconfirmed writes never offer Retry even after a late request rejection", () => {
  const tabId = "uncertain-store";
  addChatSend(tabId, { id: "bubble", correlationId: "token", content: "opaque", sentAtMs: 1700000000000 });
  observeDeliveryStatus({ tab_id: tabId, send_id: "tx", correlationId: "token", state: "submit_unconfirmed",
    error: null, ledger_id: "ledger", sequence: 3, retained_from: 1 });
  failChatSendRequest(tabId, "token");
  assert.equal(deliveryState(readChatSends(tabId)[0]), "uncertain");
  assert.equal(beginChatSendRetry(tabId, "bubble"), undefined);
});

test("unprepared attachment sends survive restart on their original bubble", () => {
  const tabId = "restored-preparation";
  addChatSend(tabId, { id: "bubble", correlationId: "token", content: "opaque", sentAtMs: 1700000000000 });
  setChatSendDraft(tabId, "bubble", { text: "opaque", attachments: ["/opaque/file"], recordedVoiceAttachments: [], capsules: {}, reply: null });
  failChatSendRequest(tabId, "bubble");
  forgetChatSendsInMemory();
  assert.equal(readChatSends(tabId).length, 1);
  assert.deepEqual(readChatSends(tabId)[0].draft?.attachments, ["/opaque/file"]);
  assert.equal(deliveryState(readChatSends(tabId)[0]), "failed");
});

test("a lost ledger failure cannot leave a stale Retry after reconnect", () => {
  const tabId = "lost-ledger";
  addChatSend(tabId, { id: "bubble", correlationId: "token", content: "opaque", sentAtMs: 1700000000000 });
  observeDeliveryStatus({ tab_id: tabId, send_id: "tx", correlationId: "token", state: "failed",
    error: null, ledger_id: "old-ledger", sequence: 3, retained_from: 1 });
  reconcileDeliveryStatuses(tabId, []);
  assert.equal(beginChatSendRetry(tabId, "bubble"), undefined);
  assert.equal(readChatSends(tabId).length, 1);
});

test("Retry mints a fresh correlation id on an insecure origin without crypto.randomUUID", () => {
  const tabId = "retry-insecure";
  const original = Object.getOwnPropertyDescriptor(globalThis, "crypto");
  Object.defineProperty(globalThis, "crypto", { configurable: true, value: { getRandomValues: (b: Uint8Array) => b.fill(7) } });
  try {
    addChatSend(tabId, { id: "bubble", correlationId: "original-token", content: "opaque", sentAtMs: 1700000000000 });
    observeDeliveryStatus({ tab_id: tabId, send_id: "first", state: "failed", ledger_id: "ledger",
      sequence: 1, retained_from: 1, error: null, correlationId: "original-token" });
    const retry = beginChatSendRetry(tabId, "bubble");
    assert(retry);
    assert.match(retry.correlationId ?? "", /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/);
  } finally {
    if (original) Object.defineProperty(globalThis, "crypto", original);
  }
});
