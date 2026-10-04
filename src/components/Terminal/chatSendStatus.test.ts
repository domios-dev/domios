import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { test } from "vitest";

import { __setInvokeForTest } from "../../lib/bridge";
import { sendTrackedChatMessage, type ChatSendStatus } from "./chatSendStatus";

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

test("frontend delivery module has no status listener, Retry, or notification path", () => {
  const source = readFileSync(resolve(process.cwd(), "src/components/Terminal/chatSendStatus.ts"), "utf8");
  assert.doesNotMatch(source, /\blisten\b/);
  assert.doesNotMatch(source, /Retry|addNotification|chat-send-status/);
});
