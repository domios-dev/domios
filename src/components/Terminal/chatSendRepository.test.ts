import assert from "node:assert/strict";
import { test } from "node:test";
import { createChatSendRepository } from "./chatSendRepository";
import { chatSendDismissalIdentity, chatSendDismissalMatchesIdentity, decodeChatSend,
  mergeChatSendRecord } from "./chatSendDurableModel";
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
  await first.persist("tab", cancellable, cancellable);
  assert.equal((await first.read("tab", "cancel"))?.deleted, true, "a stale late write cannot resurrect the tombstone");
});

test("reopened history reads the bounded settled and retry window plus all unresolved evidence", async () => {
  const database = new MemoryChatSendDatabase();
  const writer = repository(database);
  for (let index = 0; index < 100; index += 1) {
    const settled: DeliverySend = {
      id: `settled-${index}`, correlationId: `settled-token-${index}`, content: `payload-${index}`, sentAtMs: index,
      status: { send_id: `tx-${index}`, tab_id: "tab", state: "failed", error: null,
        ledger_id: "ledger", sequence: index + 1, retained_from: 1, correlationId: `settled-token-${index}` },
    };
    await writer.persist("tab", undefined, settled);
  }
  const unresolved: DeliverySend = {
    id: "unresolved", correlationId: "unresolved-token", content: "must survive", sentAtMs: 101,
    status: { send_id: "unresolved-tx", tab_id: "tab", state: "submit_unconfirmed", error: null,
      ledger_id: "ledger", sequence: 101, retained_from: 1, correlationId: "unresolved-token" },
  };
  await writer.persist("tab", undefined, unresolved);
  const historical: DeliverySend = {
    id: "dismissed-history", correlationId: "dismissed-token", content: "compact this payload", sentAtMs: 102,
    dismissed: true,
    matchedRow: { source: { source_session_id: "source-session", record: {
      kind: "structured", thread_id: "source-session", item_id: "old-provider-row",
    } }, ordinal: 8 },
  };
  await writer.persist("tab", undefined, historical);

  const reopened = repository(database);
  const hydrated = await reopened.hydrateTab("tab");

  assert.equal(hydrated.state, "ready");
  assert.equal(hydrated.rowsRead, 65);
  assert.equal(hydrated.records.length, 65);
  assert.equal(hydrated.records.some((record) => record.sendId === "settled-0"), false);
  const retryable = decodeChatSend(hydrated.records.find((record) => record.sendId === "settled-99")!);
  assert.equal(retryable?.content, "payload-99");
  assert.equal(retryable?.status?.state, "failed");
  assert.equal(decodeChatSend(hydrated.records.find((record) => record.sendId === "unresolved")!)?.status?.state,
    "submit_unconfirmed");
  assert.equal(database.records.size, 102, "older settlement payloads compact into small anti-resurrection tombstones");
  const markers = await reopened.loadDismissals("tab", [{ id: "old-provider-row", sourceSessionId: "source-session" }]);
  assert.deepEqual(markers.map((marker) => marker.sendId), ["dismissed-history"]);
  assert.equal(JSON.stringify(markers[0]).includes("compact this payload"), false);
});

test("a compacted dismissal can add a later exact echo identity without restoring its payload", async () => {
  const database = new MemoryChatSendDatabase();
  const instance = repository(database);
  const dismissed: DeliverySend = {
    id: "dismissed", correlationId: "dismissed-token", content: "large private payload", sentAtMs: 10, dismissed: true,
  };
  await instance.persist("tab", undefined, dismissed);
  assert.equal((await instance.read("tab", dismissed.id))?.deleted, true);
  assert.equal((await instance.readDismissal("tab", dismissed.id))?.echoKey, undefined);

  const withEcho = { ...dismissed, echo: { content: "historical echo", timestampMs: 42 } };
  await instance.persist("tab", dismissed, withEcho);

  const marker = await instance.readDismissal("tab", dismissed.id);
  const rowIdentity = chatSendDismissalIdentity({ id: "history-row", type: "user", content: "historical echo",
    timestamp: "1970-01-01T00:00:00.042Z" })!;
  assert.equal(chatSendDismissalMatchesIdentity(marker!, rowIdentity), true);
  assert.equal(JSON.stringify(marker).includes("large private payload"), false);
  assert.equal((await instance.read("tab", dismissed.id))?.deleted, true);
  assert.equal(database.records.size, 1);
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

test("a peer notification rereads and merges the current durable record", async () => {
  const database = new MemoryChatSendDatabase();
  const channels: Array<{ listener?: (event: MessageEvent<unknown>) => void }> = [];
  const createChannel = () => {
    const channel: { listener?: (event: MessageEvent<unknown>) => void } = {};
    channels.push(channel);
    return {
      postMessage(value: unknown) {
        for (const peer of channels) if (peer !== channel) peer.listener?.({ data: value } as MessageEvent<unknown>);
      },
      close() { channel.listener = undefined; },
      addEventListener(_type: string, listener: EventListenerOrEventListenerObject) {
        channel.listener = (event) => typeof listener === "function" ? listener(event) : listener.handleEvent(event);
      },
    };
  };
  const sender = createChatSendRepository({ openDatabase: async () => database, createChannel });
  const receiver = createChatSendRepository({ openDatabase: async () => database, createChannel });
  const pending: DeliverySend = { id: "send", correlationId: "token", content: "private payload", sentAtMs: 1 };
  await sender.persist("tab", undefined, pending);
  const local = decodeChatSend((await receiver.read("tab", "send"))!)!;
  const delivered: DeliverySend = { ...pending,
    status: { send_id: "tx", tab_id: "tab", state: "delivered", error: null,
      ledger_id: "ledger", sequence: 2, retained_from: 1, correlationId: "token" } };
  let complete!: (value: DeliverySend) => void;
  const invalidated = new Promise<DeliverySend>((resolve) => { complete = resolve; });
  receiver.subscribe((tabId, sendId) => {
    void receiver.read(tabId, sendId).then((record) => {
      const current = decodeChatSend(record!);
      if (current) complete(mergeChatSendRecord(local, local, current));
    });
  });

  await sender.persist("tab", pending, delivered);
  const merged = await invalidated;

  assert.equal(merged.status?.state, "delivered");
  assert.equal(merged.content, "private payload");
  sender.close();
  receiver.close();
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
