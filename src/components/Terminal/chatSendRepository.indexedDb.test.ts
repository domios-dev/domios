import assert from "node:assert/strict";
import { test } from "node:test";
import { openChatSendDatabase } from "./chatSendRepository";
import { connectNoticeToasts, type ToastInput } from "../../lib/notices";

function indexedDbFixture(oldVersion: number) {
  const stores = new Map<string, { keyPath: string | string[]; indexes: Map<string, string | string[]> }>();
  let aborted = false;
  let closed = false;
  const database = {
    createObjectStore(name: string, options: IDBObjectStoreParameters) {
      const indexes = new Map<string, string | string[]>();
      stores.set(name, { keyPath: options.keyPath!, indexes });
      return { createIndex: (name: string, keyPath: string | string[]) => { indexes.set(name, keyPath); } };
    },
    close: () => { closed = true; },
  };
  const request = {
    result: database,
    transaction: { abort: () => { aborted = true; } },
  } as unknown as IDBOpenDBRequest;
  const factory = {
    open(name: string, version: number) {
      assert.equal(name, "codeterm-chat-send-records");
      assert.equal(version, 2);
      queueMicrotask(() => {
        if (oldVersion !== version) {
          request.onupgradeneeded?.call(request, { oldVersion } as IDBVersionChangeEvent);
        }
        if (aborted) request.onerror?.call(request, {} as Event);
        else request.onsuccess?.call(request, {} as Event);
      });
      return request;
    },
  };
  return { factory, stores, aborted: () => aborted, closed: () => closed };
}

for (const oldVersion of [0, 1, 2]) {
  test(`native chat database open from version ${oldVersion} preserves the format boundary`, async () => {
    const fixture = indexedDbFixture(oldVersion);
    const previous = Object.getOwnPropertyDescriptor(globalThis, "indexedDB");
    Object.defineProperty(globalThis, "indexedDB", { value: fixture.factory, configurable: true });
    const notices: ToastInput[] = [];
    const disconnect = connectNoticeToasts((notice) => { notices.push(notice); });
    try {
      const database = await openChatSendDatabase();
      if (oldVersion === 1) {
        assert.equal(database, null);
        assert.equal(fixture.aborted(), true);
        assert.equal(fixture.stores.size, 0);
        assert.equal(notices.length, 1);
        assert.match(notices[0].title, /Unsupported saved chat format/);
        assert.match(notices[0].body ?? "", /version 1.*left unchanged/);
      } else {
        assert.ok(database);
        assert.equal(fixture.aborted(), false);
        assert.equal(notices.length, 0);
        if (oldVersion === 0) {
          assert.deepEqual([...fixture.stores.keys()], ["records", "tabs", "dismissals"]);
          assert.deepEqual(fixture.stores.get("records")?.indexes.get("byTabRetentionOrder"), ["tabId", "retention", "order"]);
          assert.deepEqual([...fixture.stores.get("dismissals")!.indexes.keys()], ["byTabCorrelation", "byTabMatchedRow", "byTabEcho"]);
        } else assert.equal(fixture.stores.size, 0);
        database.close();
        assert.equal(fixture.closed(), true);
      }
    } finally {
      disconnect();
      if (previous) Object.defineProperty(globalThis, "indexedDB", previous);
      else Reflect.deleteProperty(globalThis, "indexedDB");
    }
  });
}
