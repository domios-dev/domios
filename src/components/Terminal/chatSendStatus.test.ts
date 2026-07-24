import assert from "node:assert/strict";
import test from "node:test";

import { __setInvokeForTest, __setListenForTest } from "../../lib/bridge";
import {
  observeChatSendStatus,
  resetChatSendStatusForTest,
  type ChatSendStatus,
} from "./chatSendStatus";

test("a failed send is reconciled after remount exactly once", async () => {
  resetChatSendStatusForTest();
  const status: ChatSendStatus = {
    send_id: "send-1",
    pane_id: "tab-1",
    state: "failed",
    error: "submit key was not acknowledged",
  };
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
  const status: ChatSendStatus = {
    send_id: "send-2",
    pane_id: "tab-1",
    state: "failed",
    error: "turn evidence missing",
  };
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
