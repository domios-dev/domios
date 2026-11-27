import { describe, expect, it, vi } from "vitest";
import { createDraftStorage, isClosingDatabaseError } from "./draftStorage";

interface StoredRecord {
  client: string;
  scope: string;
  value: string;
}

interface FakeHandle {
  id: number;
  closed: boolean;
  closing: boolean;
  transactionCount: number;
  transactionError: Error | null;
  abortError: Error | null;
  triggerVersionChange(): void;
  triggerClose(): void;
}

function closingDomException(): DOMException {
  return new DOMException(
    "Failed to execute 'transaction' on 'IDBDatabase': The database connection is closing.",
    "InvalidStateError",
  );
}

function createFakeIndexedDb(onOpen?: (handle: FakeHandle) => void) {
  const rows = new Map<string, StoredRecord>();
  const handles: FakeHandle[] = [];
  let createdStore = false;

  const keyOf = (client: string, scope: string) => `${client}\u0000${scope}`;

  const factory: Pick<IDBFactory, "open"> = {
    open(name: string, version?: number) {
      expect(name).toBe("codeterm-composer-drafts");
      expect(version).toBe(1);
      const request = {} as IDBOpenDBRequest;
      const handle: FakeHandle = {
        id: handles.length + 1,
        closed: false,
        closing: false,
        transactionCount: 0,
        transactionError: null,
        abortError: null,
        triggerVersionChange() {
          db.onversionchange?.call(db, new Event("versionchange") as IDBVersionChangeEvent);
        },
        triggerClose() {
          handle.closing = true;
          handle.closed = true;
          db.onclose?.call(db, new Event("close"));
        },
      };
      handles.push(handle);

      const db = {
        objectStoreNames: { contains: (storeName: string) => createdStore && storeName === "drafts" },
        createObjectStore(storeName: string) {
          expect(storeName).toBe("drafts");
          createdStore = true;
          return { createIndex: () => ({}) };
        },
        close() {
          handle.closing = true;
          handle.closed = true;
        },
        transaction(storeName: string, _mode: IDBTransactionMode) {
          expect(storeName).toBe("drafts");
          handle.transactionCount += 1;
          if (handle.transactionError) throw handle.transactionError;
          if (handle.closing || handle.closed) throw closingDomException();

          const transaction = { error: null as DOMException | Error | null } as unknown as IDBTransaction;
          const fail = handle.abortError;
          const pendingResults: Array<() => void> = [];
          let committed = false;
          const scheduleFinish = () => {
            if (committed) return;
            committed = true;
            queueMicrotask(() => {
              if (fail) {
                (transaction as { error: unknown }).error = fail;
                transaction.onabort?.call(transaction, new Event("abort"));
                return;
              }
              for (const apply of pendingResults) apply();
              transaction.oncomplete?.call(transaction, new Event("complete"));
            });
          };

          (transaction as unknown as { objectStore: (name: string) => unknown }).objectStore = () => ({
            put(record: StoredRecord) {
              pendingResults.push(() => { rows.set(keyOf(record.client, record.scope), record); });
              scheduleFinish();
              return {};
            },
            delete([client, scope]: [string, string]) {
              pendingResults.push(() => { rows.delete(keyOf(client, scope)); });
              scheduleFinish();
              return {};
            },
            index(indexName: string) {
              expect(indexName).toBe("client");
              return {
                getAll(client: string) {
                  const getAllRequest = { result: [] as StoredRecord[] };
                  pendingResults.push(() => {
                    getAllRequest.result = [...rows.values()].filter(record => record.client === client);
                  });
                  scheduleFinish();
                  return getAllRequest;
                },
              };
            },
          });
          return transaction;
        },
        onversionchange: null as ((event: IDBVersionChangeEvent) => void) | null,
        onclose: null as ((event: Event) => void) | null,
      } as unknown as IDBDatabase;

      (request as { result: IDBDatabase }).result = db;
      onOpen?.(handle);

      queueMicrotask(() => {
        if (!createdStore) {
          request.onupgradeneeded?.call(request, { oldVersion: 0, newVersion: 1 } as IDBVersionChangeEvent);
        }
        request.onsuccess?.call(request, new Event("success"));
      });
      return request;
    },
  };

  return { factory, handles, rows };
}

describe("IndexedDB draft storage lifecycle", () => {
  it("classifies InvalidStateError and closing-connection errors", () => {
    expect(isClosingDatabaseError(closingDomException())).toBe(true);
    expect(isClosingDatabaseError(new Error("The database connection is closing."))).toBe(true);
    expect(isClosingDatabaseError("connection is closing")).toBe(true);
    expect(isClosingDatabaseError(new DOMException("quota exceeded", "QuotaExceededError"))).toBe(false);
    expect(isClosingDatabaseError(new Error("disk full"))).toBe(false);
  });

  it("closes and drops the cached handle on versionchange, then reopens for the next write", async () => {
    const fake = createFakeIndexedDb();
    const storage = createDraftStorage(fake.factory);

    await storage.write("tab-session-chat:t1", "first");
    expect(fake.handles).toHaveLength(1);
    expect(fake.handles[0].closed).toBe(false);

    fake.handles[0].triggerVersionChange();
    expect(fake.handles[0].closed).toBe(true);

    await storage.write("tab-session-chat:t1", "second");
    expect(fake.handles).toHaveLength(2);
    expect(fake.handles[1].closed).toBe(false);
    expect((await storage.read()).get("tab-session-chat:t1")).toBe("second");
  });

  it("drops the cached handle on close and reopens for the next write", async () => {
    const fake = createFakeIndexedDb();
    const storage = createDraftStorage(fake.factory);

    await storage.write("tab-session-chat:t1", "first");
    fake.handles[0].triggerClose();

    await storage.write("tab-session-chat:t1", "after-close");
    expect(fake.handles).toHaveLength(2);
    expect((await storage.read()).get("tab-session-chat:t1")).toBe("after-close");
  });

  it("reopens once and retries the single write when transaction() throws InvalidStateError", async () => {
    const fake = createFakeIndexedDb();
    const storage = createDraftStorage(fake.factory);

    await storage.write("tab-session-chat:t1", "initial");
    fake.handles[0].closing = true;

    await storage.write("tab-session-chat:t1", "recovered");
    expect(fake.handles).toHaveLength(2);
    expect(fake.handles[0].closed).toBe(true);
    expect(fake.handles[0].transactionCount).toBe(2);
    expect(fake.handles[1].transactionCount).toBe(1);
    expect((await storage.read()).get("tab-session-chat:t1")).toBe("recovered");
  });

  it("reopens once and retries when a transaction aborts with a closing-connection error", async () => {
    const fake = createFakeIndexedDb();
    const storage = createDraftStorage(fake.factory);

    await storage.write("tab-session-chat:t1", "initial");
    fake.handles[0].abortError = new Error("Failed to execute 'transaction' on 'IDBDatabase': The database connection is closing.");

    await storage.write("tab-session-chat:t1", "retried");
    expect(fake.handles).toHaveLength(2);
    expect(fake.handles[0].closed).toBe(true);
    expect((await storage.read()).get("tab-session-chat:t1")).toBe("retried");
  });

  it("bounds retries to one reopen and never retains a closing handle", async () => {
    const fake = createFakeIndexedDb(handle => {
      if (handle.id <= 2) handle.closing = true;
    });
    const storage = createDraftStorage(fake.factory);

    await expect(storage.write("tab-session-chat:t1", "fails-twice")).rejects.toThrow(/connection is closing/i);
    expect(fake.handles).toHaveLength(2);
    expect(fake.handles[0].closed).toBe(true);
    expect(fake.handles[1].closed).toBe(true);

    await storage.write("tab-session-chat:t1", "succeeds-on-third");
    expect(fake.handles).toHaveLength(3);
    expect((await storage.read()).get("tab-session-chat:t1")).toBe("succeeds-on-third");
  });

  it("does not reopen on non-closing errors such as QuotaExceededError", async () => {
    const fake = createFakeIndexedDb();
    const storage = createDraftStorage(fake.factory);

    await storage.write("tab-session-chat:t1", "ok");
    fake.handles[0].abortError = new DOMException("Disk full", "QuotaExceededError");

    await expect(storage.write("tab-session-chat:t1", "too-big")).rejects.toThrow("Disk full");
    expect(fake.handles).toHaveLength(1);
    expect(fake.handles[0].closed).toBe(false);
  });

  it("reopens once when read() encounters a closing handle", async () => {
    const fake = createFakeIndexedDb();
    const storage = createDraftStorage(fake.factory);

    await storage.write("tab-session-chat:t1", "saved");
    fake.handles[0].closing = true;

    const restored = await storage.read();
    expect(restored.get("tab-session-chat:t1")).toBe("saved");
    expect(fake.handles).toHaveLength(2);
    expect(fake.handles[0].closed).toBe(true);
  });

  it("logs reopen diagnostics at debug level only in DEV_BUILD", async () => {
    vi.resetModules();
    vi.doMock("../devBuild", () => ({ DEV_BUILD: true }));
    const debug = vi.spyOn(console, "debug").mockImplementation(() => {});
    try {
      const { createDraftStorage: createDevStorage } = await import("./draftStorage");
      const fake = createFakeIndexedDb();
      const storage = createDevStorage(fake.factory);
      await storage.write("tab-session-chat:t1", "initial");
      fake.handles[0].closing = true;
      await storage.write("tab-session-chat:t1", "after-reopen");
      expect(debug).toHaveBeenCalledWith(
        "[draft-storage] Reopening closing IndexedDB connection",
        expect.objectContaining({ name: "InvalidStateError" }),
      );
    } finally {
      debug.mockRestore();
      vi.doUnmock("../devBuild");
      vi.resetModules();
    }
  });
});
