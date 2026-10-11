import assert from "node:assert/strict";
import { test } from "vitest";

import { applyDeliveryStatus, deliveryState, type DeliverySend } from "./chatDeliveryModel";
import { __setInvokeForTest } from "../../lib/bridge";
import { isProvenAdmissionRefusal, sendTrackedChatMessage, type ChatSendStatus } from "./chatSendStatus";

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
