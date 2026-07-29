import assert from "node:assert/strict";
import test from "node:test";

import { __setInvokeForTest, __setListenForTest } from "../../lib/bridge";
import {
  chatSendFailureTombstoneCountForTest,
  getChatDeliveries,
  observeChatSendStatus,
  resetChatSendStatusForTest,
  sendTrackedChatMessage,
  trackChatSend,
  type ChatDelivery,
  type ChatSendStatus,
} from "./chatSendStatus";

function sendStatus(
  sendId: string,
  sequence: number,
  state: ChatSendStatus["state"] = "failed",
  retainedFrom = 1,
): ChatSendStatus {
  return {
    send_id: sendId,
    tab_id: "tab-1",
    state,
    error: state === "failed" ? `failure-${sequence}` : null,
    ledger_id: "ledger-1",
    sequence,
    retained_from: retainedFrom,
  };
}

function track(status: ChatSendStatus, message = `message-${status.sequence}`): void {
  trackChatSend(status, {
    message_id: `message-${status.sequence}`,
    message,
  });
}

function failedErrors(deliveries: ChatDelivery[]): Array<string | null> {
  return deliveries
    .filter((delivery) => delivery.status.state === "failed")
    .map((delivery) => delivery.status.error);
}

test("a failed send is reconciled and remains attributable after remount", async () => {
  resetChatSendStatusForTest();
  const status = sendStatus("send-1", 1);
  status.error = "submit_unconfirmed: submit key was not acknowledged";
  track(status, "persisted message");
  __setInvokeForTest(async <T>() => [status] as T);
  __setListenForTest(async () => () => {});
  const deliveries: ChatDelivery[] = [];

  const firstCleanup = observeChatSendStatus("tab-1", (delivery) => deliveries.push(delivery));
  await Promise.resolve();
  firstCleanup();
  const secondCleanup = observeChatSendStatus("tab-1", (delivery) => deliveries.push(delivery));
  await Promise.resolve();
  secondCleanup();

  assert.deepEqual(failedErrors(deliveries), [status.error, status.error]);
  assert.deepEqual(
    getChatDeliveries("tab-1").map((delivery) => ({
      message: delivery.message,
      state: delivery.status.state,
    })),
    [{ message: "persisted message", state: "failed" }],
  );
});

test("normal send enters the ledger and binds its optimistic message", async () => {
  resetChatSendStatusForTest();
  const pending = sendStatus("send-normal", 2, "pending");
  const commands: string[] = [];
  __setInvokeForTest(async <T>(command) => {
    commands.push(command);
    return pending as T;
  });

  await sendTrackedChatMessage("tab-1", "optimistic-1", "normal ledger message");

  assert.deepEqual(commands, ["chat_send_text"]);
  assert.deepEqual(
    getChatDeliveries("tab-1").map((delivery) => ({
      messageId: delivery.message_id,
      message: delivery.message,
      state: delivery.status.state,
    })),
    [{ messageId: "optimistic-1", message: "normal ledger message", state: "pending" }],
  );
});

test("late listener installation is cleaned after unmount", async () => {
  resetChatSendStatusForTest();
  let resolveListen: ((cleanup: () => void) => void) | null = null;
  let cleanupCalls = 0;
  let resolveCleanup!: () => void;
  const cleanupObserved = new Promise<void>((resolve) => {
    resolveCleanup = resolve;
  });
  __setInvokeForTest(async <T>() => [] as T);
  __setListenForTest(
    () =>
      new Promise((resolve) => {
        resolveListen = resolve;
      }),
  );

  const cleanup = observeChatSendStatus("tab-1", () => {});
  cleanup();
  const listenerResolver = () => resolveListen;
  listenerResolver()?.(() => {
    cleanupCalls += 1;
    resolveCleanup();
  });
  await cleanupObserved;

  assert.equal(cleanupCalls, 1);
});

test("event and reconciliation cannot surface the same terminal state twice per mount", async () => {
  resetChatSendStatusForTest();
  const pending = sendStatus("send-2", 2, "pending");
  const failed = sendStatus("send-2", 2);
  track(pending);
  let eventHandler: ((event: { payload: ChatSendStatus }) => void) | null = null;
  __setListenForTest(async <T>(_event: string, handler: (event: { payload: T }) => void) => {
    eventHandler = handler as (event: { payload: ChatSendStatus }) => void;
    return () => {};
  });
  __setInvokeForTest(async <T>() => [failed] as T);
  const deliveries: ChatDelivery[] = [];

  const cleanup = observeChatSendStatus("tab-1", (delivery) => deliveries.push(delivery));
  await Promise.resolve();
  await Promise.resolve();
  const currentHandler = () => eventHandler;
  currentHandler()?.({ payload: failed });
  cleanup();

  assert.deepEqual(failedErrors(deliveries), ["failure-2"]);
});

test("a terminal event held before send-id binding is surfaced after binding", async () => {
  resetChatSendStatusForTest();
  const pending = sendStatus("send-3", 3, "pending");
  const failed = sendStatus("send-3", 3);
  let eventHandler: ((event: { payload: ChatSendStatus }) => void) | null = null;
  __setListenForTest(async <T>(_event: string, handler: (event: { payload: T }) => void) => {
    eventHandler = handler as (event: { payload: ChatSendStatus }) => void;
    return () => {};
  });
  __setInvokeForTest(async <T>() => [] as T);
  const deliveries: ChatDelivery[] = [];
  const cleanup = observeChatSendStatus("tab-1", (delivery) => deliveries.push(delivery));
  await Promise.resolve();

  const currentHandler = () => eventHandler;
  currentHandler()?.({ payload: failed });
  assert.deepEqual(deliveries, []);
  track(pending, "race-safe message");
  cleanup();

  assert.deepEqual(failedErrors(deliveries), ["failure-3"]);
  assert.equal(deliveries[0]?.message, "race-safe message");
});

test("backend retention window bounds retained failures", async () => {
  resetChatSendStatusForTest();
  __setInvokeForTest(async <T>() => [] as T);
  __setListenForTest(async () => () => {});
  const deliveries: ChatDelivery[] = [];
  const cleanup = observeChatSendStatus("tab-1", (delivery) => deliveries.push(delivery));
  await Promise.resolve();

  for (let sequence = 1; sequence <= 257; sequence += 1) {
    track(sendStatus(`send-${sequence}`, sequence, "failed", Math.max(1, sequence - 255)));
  }
  cleanup();

  assert.equal(chatSendFailureTombstoneCountForTest(), 256);
  assert.equal(getChatDeliveries("tab-1").length, 256);
  assert.equal(failedErrors(deliveries).length, 257);
  assert.ok(!getChatDeliveries("tab-1").some((delivery) => delivery.status.send_id === "send-1"));
});
