import assert from "node:assert/strict";
import { test } from "node:test";
import { createChatSendRepository } from "./chatSendRepository";
import { decodeChatSend } from "./chatSendDurableModel";
import { MemoryChatSendDatabase } from "./chatSendRepository.testSupport";
import type { DeliverySend } from "./chatDeliveryModel";

function repository(database: MemoryChatSendDatabase) {
  return createChatSendRepository({
    openDatabase: async () => database,
    createChannel: () => null,
  });
}

test("an empty new-schema database hydrates as a clean ready ledger", async () => {
  const database = new MemoryChatSendDatabase();
  const instance = repository(database);

  const hydrated = await instance.hydrateTab("tab");

  assert.equal(hydrated.state, "ready");
  assert.deepEqual(hydrated.records, []);
  assert.equal(database.writes.length, 0);
});

test("hydration preserves existing delivery status and draft data", async () => {
  const existing: DeliverySend = {
    id: "send", correlationId: "token", content: "payload", sentAtMs: 1,
    status: { send_id: "tx", tab_id: "tab", state: "delivered", error: null, ledger_id: "ledger", sequence: 4,
      retained_from: 1, correlationId: "token" },
    draft: { text: "new draft", attachments: ["/new"], recordedVoiceAttachments: [], capsules: {}, reply: null },
  };
  const database = new MemoryChatSendDatabase();
  await database.write("tab", undefined, existing);
  const instance = repository(database);

  const hydrated = await instance.hydrateTab("tab");

  const restored = decodeChatSend(hydrated.records[0]!);
  assert.equal(restored?.status?.state, "delivered");
  assert.deepEqual(restored?.draft, existing.draft);
});

test("connection writes preserve delivery evidence and a removed row cannot return", async () => {
  const database = new MemoryChatSendDatabase();
  const first = repository(database);
  const second = repository(database);
  const pending: DeliverySend = { id: "settled", correlationId: "token", content: "payload", sentAtMs: 1 };
  const delivered: DeliverySend = {
    ...pending,
    matchedRow: { source: { source_session_id: "source", record: { kind: "structured", thread_id: "source", item_id: "row" } }, ordinal: 1 },
    status: { send_id: "tx", tab_id: "tab", state: "delivered", error: null, ledger_id: "ledger", sequence: 8,
      retained_from: 1, correlationId: "token" },
  };
  const stale: DeliverySend = {
    ...pending,
    status: { send_id: "tx", tab_id: "tab", state: "written", error: null, ledger_id: "ledger", sequence: 2,
      retained_from: 1, correlationId: "token" },
  };

  await first.persist("tab", undefined, pending);
  await second.persist("tab", pending, delivered);
  await first.persist("tab", pending, stale);

  const settled = await second.read("tab", "settled");
  assert.equal(decodeChatSend(settled!)?.status?.state, "delivered");
  assert.deepEqual(decodeChatSend(settled!)?.matchedRow, delivered.matchedRow);

  const cancellable: DeliverySend = { id: "cancel", correlationId: "cancel-token", content: "unwritten", sentAtMs: 2 };
  await first.persist("tab", undefined, cancellable);
  await second.remove("tab", "cancel", cancellable);
  await first.persist("tab", cancellable, cancellable);

  const tombstone = await first.read("tab", "cancel");
  assert.equal(tombstone?.deleted, true);
  assert.equal(decodeChatSend(tombstone!), null);
});

test("cross-connection notifications contain only keys and publish only changed records", async () => {
  const database = new MemoryChatSendDatabase();
  const messages: unknown[] = [];
  const instance = createChatSendRepository({
    openDatabase: async () => database,
    createChannel: () => ({
      postMessage: (value) => messages.push(value),
      close: () => {},
      addEventListener: () => {},
    }),
  });
  const send: DeliverySend = { id: "send", correlationId: "token", content: "private payload", sentAtMs: 1 };

  await instance.persist("tab", undefined, send);
  await instance.persist("tab", send, send);
  await instance.remove("tab", "send", send);
  await instance.remove("tab", "send", send);

  assert.deepEqual(messages, [
    { tabId: "tab", sendId: "send" },
    { tabId: "tab", sendId: "send" },
  ]);
  assert.equal(JSON.stringify(messages).includes("private payload"), false);
});

test("unavailable storage and read failures settle without gating callers", async () => {
  const unavailable = createChatSendRepository({
    openDatabase: async () => null,
    createChannel: () => null,
  });
  const hydrated = await unavailable.hydrateTab("tab");
  assert.equal(hydrated.state, "unavailable");
  await unavailable.persist("tab", undefined, { id: "send", content: "payload", sentAtMs: 1 });
  assert.equal(unavailable.state(), "unavailable");

  const database = new MemoryChatSendDatabase();
  database.failRead = true;
  const instance = repository(database);
  assert.equal(await instance.read("tab", "missing"), undefined);
  assert.equal(instance.state(), "error");
});
