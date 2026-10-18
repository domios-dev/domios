import {
  decodeChatSend,
  encodeChatSend,
  encodeChatSendTombstone,
  isDurableRecord,
  mergeChatSendRecord,
  orderChatSendRecords,
  sameDurableRecord,
  type ChatSendDurableRecord,
} from "./chatSendDurableModel";
import type { DeliverySend } from "./chatDeliveryModel";

const DATABASE_NAME = "codeterm-chat-send-records";
const DATABASE_VERSION = 1;
const RECORDS = "records";
const TAB_STATE = "tabs";
const OPEN_DEADLINE_MS = 4_000;
const TRANSACTION_DEADLINE_MS = 5_000;
const CHANNEL_NAME = "codeterm-chat-send-record-changes";

export type ChatSendPersistenceState = "pending" | "ready" | "unavailable" | "error";

export interface ChatSendHydration {
  records: readonly ChatSendDurableRecord[];
  state: ChatSendPersistenceState;
}

export interface ChatSendRecordDatabase {
  loadTab(tabId: string): Promise<ChatSendDurableRecord[]>;
  read(tabId: string, sendId: string): Promise<ChatSendDurableRecord | undefined>;
  write(tabId: string, previous: DeliverySend | undefined, next: DeliverySend): Promise<ChatSendRecordWrite>;
  remove(tabId: string, sendId: string, previous: DeliverySend | undefined): Promise<ChatSendRecordWrite>;
  isUsable?(): boolean;
  close(): void;
}

export interface ChatSendRecordWrite {
  record: ChatSendDurableRecord;
  changed: boolean;
}

export interface ChatSendRepositoryPort {
  hydrateTab(tabId: string): Promise<ChatSendHydration>;
  persist(tabId: string, previous: DeliverySend | undefined, next: DeliverySend): Promise<void>;
  remove(tabId: string, sendId: string, previous: DeliverySend | undefined): Promise<void>;
  read(tabId: string, sendId: string): Promise<ChatSendDurableRecord | undefined>;
  subscribe(listener: (tabId: string, sendId: string) => void): () => void;
  state(): ChatSendPersistenceState;
  close(): void;
}

export interface ChatSendRepositoryDependencies {
  openDatabase?: () => Promise<ChatSendRecordDatabase | null>;
  createChannel?: () => ChangeChannel | null;
}

type ChangeChannel = Pick<BroadcastChannel, "postMessage" | "close" | "addEventListener">;

export function createChatSendRepository(dependencies: ChatSendRepositoryDependencies = {}): ChatSendRepositoryPort {
  const openDatabase = dependencies.openDatabase ?? openIndexedDb;
  const channel = createChangeChannel(dependencies.createChannel);
  const listeners = new Set<(tabId: string, sendId: string) => void>();
  let database: ChatSendRecordDatabase | null = null;
  let opening: Promise<ChatSendRecordDatabase | null> | null = null;
  let persistenceState: ChatSendPersistenceState = "pending";

  channel?.addEventListener("message", (event: MessageEvent<unknown>) => {
    const value = event.data as { tabId?: unknown; sendId?: unknown } | null;
    if (typeof value?.tabId !== "string" || typeof value.sendId !== "string") return;
    for (const listener of listeners) listener(value.tabId, value.sendId);
  });

  const connection = async () => {
    if (database?.isUsable?.() === false) {
      database.close();
      database = null;
    }
    if (database) return database;
    if (opening) return opening;
    opening = openDatabase().then((opened) => {
      database = opened;
      return opened;
    }).catch(() => null).finally(() => { opening = null; });
    return opening;
  };

  return {
    async hydrateTab(tabId) {
      const db = await connection();
      if (!db) {
        persistenceState = "unavailable";
        return { records: [], state: persistenceState };
      }
      try {
        const records = await db.loadTab(tabId);
        persistenceState = "ready";
        return { records, state: persistenceState };
      } catch {
        persistenceState = "error";
        return { records: [], state: persistenceState };
      }
    },
    async persist(tabId, previous, next) {
      let db = await connection();
      if (!db) {
        persistenceState = "unavailable";
        return;
      }
      try {
        const result = await db.write(tabId, previous, next);
        persistenceState = "ready";
        if (result.changed) channel?.postMessage({ tabId, sendId: next.id });
      } catch {
        persistenceState = "error";
      }
    },
    async remove(tabId, sendId, previous) {
      const db = await connection();
      if (!db) {
        persistenceState = "unavailable";
        return;
      }
      try {
        const result = await db.remove(tabId, sendId, previous);
        persistenceState = "ready";
        if (result.changed) channel?.postMessage({ tabId, sendId });
      } catch {
        persistenceState = "error";
      }
    },
    async read(tabId, sendId) {
      const db = await connection();
      if (!db) return;
      try {
        return await db.read(tabId, sendId);
      } catch {
        persistenceState = "error";
      }
    },
    subscribe(listener) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    state: () => persistenceState,
    close() {
      database?.close();
      database = null;
      channel?.close();
      listeners.clear();
    },
  };
}

class IndexedDbChatSendDatabase implements ChatSendRecordDatabase {
  private closed = false;

  constructor(private readonly database: IDBDatabase) {
    database.onversionchange = () => this.close();
  }

  loadTab(tabId: string): Promise<ChatSendDurableRecord[]> {
    return this.run("readonly", (store, _tabs, complete) => {
      const request = store.index("byTab").getAll(tabId);
      request.onsuccess = () => complete(orderChatSendRecords((request.result as unknown[]).filter(isDurableRecord)));
    });
  }

  read(tabId: string, sendId: string): Promise<ChatSendDurableRecord | undefined> {
    return this.run("readonly", (store, _tabs, complete) => {
      const request = store.get([tabId, sendId]);
      request.onsuccess = () => complete(isDurableRecord(request.result) ? request.result : undefined);
    });
  }

  write(tabId: string, previous: DeliverySend | undefined, next: DeliverySend): Promise<ChatSendRecordWrite> {
    return this.run("readwrite", (store, tabs, complete, fail) => {
      const request = store.get([tabId, next.id]);
      request.onsuccess = () => {
        const prior = isDurableRecord(request.result) ? request.result : undefined;
        if (prior?.deleted) {
          complete({ record: prior, changed: false });
          return;
        }
        this.allocateOrder(tabs, tabId, prior?.order, undefined, (order) => {
          const current = prior ? decodeChatSend(prior) ?? undefined : undefined;
          const merged = mergeChatSendRecord(current, previous, next);
          const record = encodeChatSend(tabId, merged, (prior?.revision ?? -1) + 1, order);
          if (prior && sameDurableRecord(prior, record)) {
            complete({ record: prior, changed: false });
            return;
          }
          const put = store.put(record);
          put.onsuccess = () => complete({ record, changed: true });
          put.onerror = () => fail(put.error ?? new Error("chat send record write failed"));
        }, fail);
      };
      request.onerror = () => fail(request.error ?? new Error("chat send record read failed"));
    }, true);
  }

  remove(tabId: string, sendId: string, _previous: DeliverySend | undefined): Promise<ChatSendRecordWrite> {
    return this.run("readwrite", (store, tabs, complete, fail) => {
      const request = store.get([tabId, sendId]);
      request.onsuccess = () => {
        const prior = isDurableRecord(request.result) ? request.result : undefined;
        if (prior?.deleted) {
          complete({ record: prior, changed: false });
          return;
        }
        const current = prior ? decodeChatSend(prior) : null;
        if (current?.status || current?.requestUncertain) {
          complete({ record: prior!, changed: false });
          return;
        }
        this.allocateOrder(tabs, tabId, prior?.order, undefined, (order) => {
          const tombstone = encodeChatSendTombstone(tabId, sendId, (prior?.revision ?? -1) + 1, order);
          const put = store.put(tombstone);
          put.onsuccess = () => complete({ record: tombstone, changed: true });
          put.onerror = () => fail(put.error ?? new Error("chat send tombstone write failed"));
        }, fail);
      };
      request.onerror = () => fail(request.error ?? new Error("chat send record read failed"));
    }, true);
  }

  close(): void {
    this.closed = true;
    this.database.close();
  }

  isUsable(): boolean {
    return !this.closed;
  }

  private run<T>(
    mode: IDBTransactionMode,
    queue: (store: IDBObjectStore, tabs: IDBObjectStore | undefined,
      complete: (value: T) => void, fail: (error: Error) => void) => void,
    withTabState = false,
  ): Promise<T> {
    return new Promise<T>((resolve, reject) => {
      let transaction!: IDBTransaction;
      let value: T;
      let hasValue = false;
      let settled = false;
      const finish = (error?: Error) => {
        if (settled) return;
        settled = true;
        clearTimeout(deadline);
        if (error) reject(error);
        else if (hasValue) resolve(value);
        else reject(new Error("chat send transaction completed without a result"));
      };
      const deadline = setTimeout(() => {
        try { transaction.abort(); } catch { finish(new Error("chat send transaction deadline exceeded")); }
      }, TRANSACTION_DEADLINE_MS);
      try {
        transaction = this.database.transaction(withTabState ? [RECORDS, TAB_STATE] : RECORDS, mode);
        transaction.oncomplete = () => finish();
        transaction.onerror = () => finish(transaction.error ?? new Error("chat send transaction failed"));
        transaction.onabort = () => finish(transaction.error ?? new Error("chat send transaction aborted"));
        queue(transaction.objectStore(RECORDS), withTabState ? transaction.objectStore(TAB_STATE) : undefined,
          (next) => { value = next; hasValue = true; }, (error) => {
          try { transaction.abort(); } catch { finish(error); }
        });
      } catch (error) {
        finish(error instanceof Error ? error : new Error(String(error)));
      }
    });
  }

  private allocateOrder(
    tabs: IDBObjectStore | undefined,
    tabId: string,
    existingOrder: number | undefined,
    preferredOrder: number | undefined,
    complete: (order: number) => void,
    fail: (error: Error) => void,
  ): void {
    if (!tabs) {
      fail(new Error("chat send order store unavailable"));
      return;
    }
    const read = tabs.get(tabId);
    read.onsuccess = () => {
      const current = read.result as { nextOrder?: unknown } | undefined;
      const nextOrder = typeof current?.nextOrder === "number" && Number.isSafeInteger(current.nextOrder)
        ? current.nextOrder
        : 0;
      const order = existingOrder ?? preferredOrder ?? nextOrder;
      const write = tabs.put({ tabId, nextOrder: Math.max(nextOrder, order + 1) });
      write.onsuccess = () => complete(order);
      write.onerror = () => fail(write.error ?? new Error("chat send order write failed"));
    };
    read.onerror = () => fail(read.error ?? new Error("chat send order read failed"));
  }
}

function openIndexedDb(): Promise<ChatSendRecordDatabase | null> {
  return new Promise((resolve) => {
    if (typeof indexedDB === "undefined") {
      resolve(null);
      return;
    }
    let request: IDBOpenDBRequest;
    try {
      request = indexedDB.open(DATABASE_NAME, DATABASE_VERSION);
    } catch {
      resolve(null);
      return;
    }
    let settled = false;
    const finish = (value: ChatSendRecordDatabase | null) => {
      if (settled) {
        value?.close();
        return;
      }
      settled = true;
      clearTimeout(deadline);
      resolve(value);
    };
    const deadline = setTimeout(() => finish(null), OPEN_DEADLINE_MS);
    request.onupgradeneeded = () => {
      const database = request.result;
      if (!database.objectStoreNames.contains(RECORDS)) {
        const store = database.createObjectStore(RECORDS, { keyPath: ["tabId", "sendId"] });
        store.createIndex("byTab", "tabId", { unique: false });
      }
      if (!database.objectStoreNames.contains(TAB_STATE)) database.createObjectStore(TAB_STATE, { keyPath: "tabId" });
    };
    request.onblocked = () => finish(null);
    request.onerror = () => finish(null);
    request.onsuccess = () => finish(new IndexedDbChatSendDatabase(request.result));
  });
}

function createChangeChannel(factory?: ChatSendRepositoryDependencies["createChannel"]): ChangeChannel | null {
  try {
    const channel = factory ? factory() : typeof window === "undefined" || typeof window.BroadcastChannel === "undefined"
      ? null
      : new window.BroadcastChannel(CHANNEL_NAME);
    return channel;
  } catch {
    return null;
  }
}
