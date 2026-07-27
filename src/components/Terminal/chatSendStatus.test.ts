import assert from "node:assert/strict";
import test from "node:test";

import { __setInvokeForTest, __setListenForTest } from "../../lib/bridge";
import {
  chatSendFailureTombstoneCountForTest,
  observeChatSendStatus,
  resetChatSendStatusForTest,
  type ChatSendStatus,
} from "./chatSendStatus";

function failedStatus(
  sendId: string,
  sequence: number,
  retainedFrom = 1,
): ChatSendStatus {
  return {
    send_id: sendId,
    tab_id: "tab-1",
    state: "failed",
    error: `failure-${sequence}`,
    ledger_id: "ledger-1",
    sequence,
    retained_from: retainedFrom,
  };
}

test("a failed send is reconciled after remount exactly once", async () => {
  resetChatSendStatusForTest();
  const status = failedStatus("send-1", 1);
  status.error = "submit key was not acknowledged";
  __setInvokeForTest(async <T>() => [status] as T);
  __setListenForTest(async () => () => {});
  const failures: string[] = [];
  let resolveFailure!: () => void;
  const failureObserved = new Promise<void>((resolve) => {
    resolveFailure = resolve;
  });

  const firstCleanup = observeChatSendStatus("tab-1", (error) => {
    failures.push(error);
    resolveFailure();
  });
  await failureObserved;
  firstCleanup();
  const secondCleanup = observeChatSendStatus("tab-1", (error) => failures.push(error));
  await Promise.resolve();
  await Promise.resolve();
  secondCleanup();

  assert.deepEqual(failures, ["submit key was not acknowledged"]);
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

test("event and reconciliation cannot surface the same failure twice", async () => {
  resetChatSendStatusForTest();
  const status = failedStatus("send-2", 2);
  status.error = "turn evidence missing";
  let eventHandler: ((event: { payload: ChatSendStatus }) => void) | null = null;
  __setListenForTest(async <T>(_event: string, handler: (event: { payload: T }) => void) => {
    eventHandler = handler as (event: { payload: ChatSendStatus }) => void;
    return () => {};
  });
  let resolveFailure!: () => void;
  const failureObserved = new Promise<void>((resolve) => {
    resolveFailure = resolve;
  });
  __setInvokeForTest(async <T>() => [status] as T);
  const failures: string[] = [];

  const cleanup = observeChatSendStatus("tab-1", (error) => {
    failures.push(error);
    resolveFailure();
  });
  await failureObserved;
  const currentHandler = () => eventHandler;
  currentHandler()?.({ payload: status });
  cleanup();

  assert.deepEqual(failures, ["turn evidence missing"]);
});

test("listener rejection still performs durable reconciliation without rejection leakage", async () => {
  resetChatSendStatusForTest();
  const status = failedStatus("send-3", 3);
  let resolveFailure!: () => void;
  const failureObserved = new Promise<void>((resolve) => {
    resolveFailure = resolve;
  });
  __setListenForTest(async () => {
    throw new Error("listener unavailable");
  });
  __setInvokeForTest(async <T>() => [status] as T);
  const failures: string[] = [];

  const cleanup = observeChatSendStatus("tab-1", (error) => {
    failures.push(error);
    resolveFailure();
  });
  await failureObserved;
  cleanup();

  assert.deepEqual(failures, ["failure-3"]);
});

test("backend retention window bounds tombstones without re-alerting retained failures", async () => {
  resetChatSendStatusForTest();
  let eventHandler: ((event: { payload: ChatSendStatus }) => void) | null = null;
  __setListenForTest(async <T>(_event: string, handler: (event: { payload: T }) => void) => {
    eventHandler = handler as (event: { payload: ChatSendStatus }) => void;
    return () => {};
  });
  const retained = Array.from({ length: 256 }, (_, index) =>
    failedStatus(`send-${index + 2}`, index + 2, 2),
  );
  let invokeCalls = 0;
  let resolveSecondInvoke!: () => void;
  const secondInvokeObserved = new Promise<void>((resolve) => {
    resolveSecondInvoke = resolve;
  });
  __setInvokeForTest(async <T>() => {
    invokeCalls += 1;
    if (invokeCalls === 2) resolveSecondInvoke();
    return retained as T;
  });
  const failures: string[] = [];
  let resolveInitialReconciliation!: () => void;
  const initialReconciliationObserved = new Promise<void>((resolve) => {
    resolveInitialReconciliation = resolve;
  });
  const cleanup = observeChatSendStatus("tab-1", (error) => {
    failures.push(error);
    if (failures.length === retained.length) resolveInitialReconciliation();
  });
  await initialReconciliationObserved;

  const currentHandler = () => eventHandler;
  currentHandler()?.({ payload: failedStatus("send-1", 1, 1) });
  for (const status of retained) currentHandler()?.({ payload: status });
  cleanup();

  const remount = observeChatSendStatus("tab-1", (error) => failures.push(error));
  await secondInvokeObserved;
  await Promise.resolve();
  remount();

  assert.equal(chatSendFailureTombstoneCountForTest(), 256);
  assert.equal(failures.length, 256);
  assert.ok(!failures.includes("failure-1"));
});
