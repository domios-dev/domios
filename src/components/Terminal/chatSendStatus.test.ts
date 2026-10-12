import assert from "node:assert/strict";
import { test } from "vitest";

import { applyDeliveryStatus, deliveryState, type DeliverySend } from "./chatDeliveryModel";
import { __setInvokeForTest } from "../../lib/bridge";
import { isProvenAdmissionRefusal, reconcileTrackedChatMessages, sendTrackedChatMessage, type ChatSendStatus } from "./chatSendStatus";
import { addChatSend, beginChatSendRetry, forgetChatSendsInMemory, observeDeliveryStatus, readChatSends } from "./chatSendStore";

function heldStatus(): ChatSendStatus {
  return {
    send_id: "send-1",
    tab_id: "tab-1",
    state: "held",
    error: null,
    ledger_id: "ledger-1",
    sequence: 1,
    retained_from: 1,
    correlationId: "message-1",
  };
}

test("restart reconciliation wires durable states onto each persisted held bubble without resubmitting", async () => {
  for (const state of ["held", "failed", "delivered", "submit_unconfirmed"] as const) {
    const tabId = `restart-${state}`;
    const prior = { ...heldStatus(), tab_id: tabId };
    addChatSend(tabId, { id: "bubble", correlationId: "message-1", content: "payload", sentAtMs: 1 });
    observeDeliveryStatus(prior);
    forgetChatSendsInMemory();
    const restored = { ...prior, ledger_id: "new-daemon", state };
    const calls: unknown[] = [];
    __setInvokeForTest(async <T>(command: string, args?: unknown) => {
      calls.push({ command, args });
      return [restored] as T;
    });
    await reconcileTrackedChatMessages(tabId);
    forgetChatSendsInMemory();
    const sends = readChatSends(tabId);
    assert.equal(sends.length, 1);
    assert.equal(sends[0].id, "bubble");
    assert.equal(sends[0].status?.state, state);
    assert.equal(sends[0].status?.ledger_id, "new-daemon");
    assert.deepEqual(calls, [{ command: "chat_send_status", args: { tabId } }]);
    assert.equal(beginChatSendRetry(tabId, "bubble") !== undefined, state === "failed");
  }
});

test("an absent receipt clears stale held state to uncertainty without inventing Retry", async () => {
  const tabId = "missing-restart-receipt";
  addChatSend(tabId, { id: "bubble", correlationId: "message-1", content: "payload", sentAtMs: 1 });
  observeDeliveryStatus({ ...heldStatus(), tab_id: tabId });
  forgetChatSendsInMemory();
  __setInvokeForTest(async <T>() => [] as T);
  await reconcileTrackedChatMessages(tabId);
  assert.equal(deliveryState(readChatSends(tabId)[0]), "uncertain");
  assert.equal(beginChatSendRetry(tabId, "bubble"), undefined);
});

test("held send enters the backend ledger once", async () => {
  const calls: Array<{ command: string; args: unknown }> = [];
  const accepted = heldStatus();
  __setInvokeForTest(async <T>(command: string, args?: unknown) => {
    calls.push({ command, args });
    return accepted as T;
  });

  const result = await sendTrackedChatMessage("tab-1", "message-1", "hello");

  assert.deepEqual(result, accepted);
  assert.deepEqual(calls, [{
    command: "chat_send_text",
    args: { tabId: "tab-1", text: "hello", clientToken: "message-1" },
  }]);
});

test("request rejection is logged and rethrown without a UI status channel", async () => {
  const failure = new Error("transport unavailable");
  __setInvokeForTest(async () => {
    throw failure;
  });
  const previousWarn = console.warn;
  const warnings: unknown[][] = [];
  console.warn = (...args: unknown[]) => warnings.push(args);
  try {
    await assert.rejects(
      sendTrackedChatMessage("tab-1", "message-2", "hello"),
      failure,
    );
  } finally {
    console.warn = previousWarn;
  }

  assert.equal(warnings.length, 1);
  assert.match(String(warnings[0]?.[0]), /\[chat-send\]/);
});

test("only proven pre-admission refusal allows Retry", () => {
  assert.equal(isProvenAdmissionRefusal(new Error("user send capacity: 32 unsettled admissions")), true);
  assert.equal(isProvenAdmissionRefusal(new Error("admission_unknown: durability could not be qualified")), false);
  assert.equal(isProvenAdmissionRefusal(new TypeError("Failed to fetch")), false);
  assert.equal(isProvenAdmissionRefusal(new Error("connection reset")), false);
});

test("reconnect resolves a response-less refused admission using durable absence proof", async () => {
  const tabId = "offline-admission";
  addChatSend(tabId, { id: "offline-bubble", correlationId: "offline-token", content: "opaque", sentAtMs: 1 });
  const calls: string[] = [];
  __setInvokeForTest(async <T>(command: string) => {
    calls.push(command);
    if (command === "chat_send_text") throw new TypeError("opaque-network-failure");
    if (command === "chat_send_status") return [] as T;
    if (command === "chat_send_admissions") return [{ correlationId: "offline-token", evidence: "absent" }] as T;
    throw new Error(command);
  });
  await assert.rejects(sendTrackedChatMessage(tabId, "offline-token", "opaque"));
  await reconcileTrackedChatMessages(tabId);
  forgetChatSendsInMemory();
  assert.equal(deliveryState(readChatSends(tabId)[0]), "failed");
  assert.equal(calls.filter((command) => command === "chat_send_text").length, 1);
  assert.notEqual(beginChatSendRetry(tabId, "offline-bubble"), undefined);
});

test("reconnect leaves an ambiguous response uncertain even when no admission is retained", async () => {
  const tabId = "ambiguous-admission";
  addChatSend(tabId, { id: "ambiguous-bubble", correlationId: "ambiguous-token", content: "opaque", sentAtMs: 1 });
  __setInvokeForTest(async <T>(command: string) => {
    if (command === "chat_send_text") throw new Error("opaque-ambiguous-response");
    if (command === "chat_send_status") return [] as T;
    return [{ correlationId: "ambiguous-token", evidence: "absent" }] as T;
  });
  await assert.rejects(sendTrackedChatMessage(tabId, "ambiguous-token", "opaque"));
  await reconcileTrackedChatMessages(tabId);
  assert.equal(deliveryState(readChatSends(tabId)[0]), "uncertain");
  assert.equal(beginChatSendRetry(tabId, "ambiguous-bubble"), undefined);
});


test("model command hold shows queued, then sending and delivered on the same bubble", async () => {
  let submissions = 0;
  const held = { ...heldStatus(), pending_reason: "awaiting_model_command" };
  __setInvokeForTest(async <T>() => {
    submissions += 1;
    return held as T;
  });
  const receipt = await sendTrackedChatMessage("tab-1", "message-1", "opaque payload");
  const original: DeliverySend = {
    id: "bubble", correlationId: "message-1", content: "opaque payload", sentAtMs: 1000,
  };
  const queued = applyDeliveryStatus(original, receipt);
  assert.equal(deliveryState(queued), "queued");
  const deadline = applyDeliveryStatus(queued, { ...receipt, pending_reason: "model_command_completion_unconfirmed" });
  assert.equal(deliveryState(deadline), "queued");
  const written = applyDeliveryStatus(deadline, { ...receipt, state: "written", pending_reason: undefined });
  assert.equal(deliveryState(written), "sending");
  const delivered = applyDeliveryStatus(written, { ...receipt, state: "delivered", pending_reason: undefined });
  assert.equal(deliveryState(delivered), "delivered");
  assert.equal(delivered.id, original.id);
  assert.equal(submissions, 1);
});
