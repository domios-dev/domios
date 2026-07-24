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
  __setInvokeForTest(async () => status);
  __setListenForTest(async () => () => {});
  const failures: string[] = [];

  const firstCleanup = observeChatSendStatus("tab-1", (error) => failures.push(error));
  await Promise.resolve();
  firstCleanup();
  const secondCleanup = observeChatSendStatus("tab-1", (error) => failures.push(error));
  await Promise.resolve();
  secondCleanup();

  assert.deepEqual(failures, ["submit key was not acknowledged"]);
});

test("late listener installation is cleaned after unmount", async () => {
  resetChatSendStatusForTest();
  let resolveListen: ((cleanup: () => void) => void) | null = null;
  let cleanupCalls = 0;
  __setInvokeForTest(async () => null);
  __setListenForTest(
    () =>
      new Promise((resolve) => {
        resolveListen = resolve;
      }),
  );

  const cleanup = observeChatSendStatus("tab-1", () => {});
  cleanup();
  resolveListen?.(() => {
    cleanupCalls += 1;
  });
  await Promise.resolve();

  assert.equal(cleanupCalls, 1);
});
