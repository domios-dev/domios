import {
  chatSendRetention,
  chatSendDismissalMatchedRowKeys,
  decodeChatSend,
  encodeChatSendDismissalMarker,
  encodeChatSend,
  encodeChatSendTombstone,
  isChatSendDismissalMarker,
  isDurableRecord,
  mergeChatSendDismissalMarker,
  mergeChatSendRecord,
  orderChatSendRecords,
  sameChatSendDismissalMarker,
  sameDurableRecord,
  type ChatSendDismissalIdentity,
  type ChatSendDismissalMarker,
  type ChatSendDurableRecord,
} from "./chatSendDurableModel";
import { publishNotice } from "../../lib/notices";
import type { DeliverySend } from "./chatDeliveryModel";

const DATABASE_NAME = "codeterm-chat-send-records";
const DATABASE_VERSION = 2;
const RECORDS = "records";
const TAB_STATE = "tabs";
const DISMISSALS = "dismissals";
const MAX_SETTLED_RECORDS = 64;
const OPEN_DEADLINE_MS = 4_000;
const TRANSACTION_DEADLINE_MS = 5_000;
const CHANNEL_NAME = "codeterm-chat-send-record-changes";

export type ChatSendPersistenceState = "pending" | "ready" | "unavailable" | "error";

export interface ChatSendHydration {
  records: readonly ChatSendDurableRecord[];
  state: ChatSendPersistenceState;
  rowsRead: number;
}

export interface ChatSendRecordLoad {
  records: ChatSendDurableRecord[];
  rowsRead: number;
}

export interface ChatSendRecordDatabase {
  loadTab(tabId: string): Promise<ChatSendRecordLoad>;
  loadDismissals(tabId: string, identities: readonly ChatSendDismissalIdentity[]): Promise<ChatSendDismissalMarker[]>;
  readDismissal(tabId: string, sendId: string): Promise<ChatSendDismissalMarker | undefined>;
  dismiss(tabId: string, send: DeliverySend): Promise<boolean>;
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
  loadDismissals(tabId: string, identities: readonly ChatSendDismissalIdentity[]): Promise<ChatSendDismissalMarker[]>;
  persistDismissal(tabId: string, send: DeliverySend): Promise<void>;
  persist(tabId: string, previous: DeliverySend | undefined, next: DeliverySend): Promise<void>;
  remove(tabId: string, sendId: string, previous: DeliverySend | undefined): Promise<void>;
  read(tabId: string, sendId: string): Promise<ChatSendDurableRecord | undefined>;
  readDismissal(tabId: string, sendId: string): Promise<ChatSendDismissalMarker | undefined>;
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
  const openDatabase = dependencies.openDatabase ?? openChatSendDatabase;
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
        return { records: [], state: persistenceState, rowsRead: 0 };
      }
      try {
        const { records, rowsRead } = await db.loadTab(tabId);
        persistenceState = "ready";
        return { records, state: persistenceState, rowsRead };
      } catch {
        persistenceState = "error";
        return { records: [], state: persistenceState, rowsRead: 0 };
      }
    },
    async loadDismissals(tabId, identities) {
      const db = await connection();
      if (!db) {
        persistenceState = "unavailable";
        return [];
      }
      try {
        const markers = await db.loadDismissals(tabId, identities);
        persistenceState = "ready";
        return markers;
      } catch {
        persistenceState = "error";
        return [];
      }
    },
    async persistDismissal(tabId, send) {
      const db = await connection();
      if (!db) {
        persistenceState = "unavailable";
        return;
      }
      try {
        const changed = await db.dismiss(tabId, send);
        persistenceState = "ready";
        if (changed) channel?.postMessage({ tabId, sendId: send.id });
      } catch {
        persistenceState = "error";
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
    async readDismissal(tabId, sendId) {
      const db = await connection();
      if (!db) return;
      try {
        return await db.readDismissal(tabId, sendId);
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

  loadTab(tabId: string): Promise<ChatSendRecordLoad> {
    return this.run("readonly", (store, _tabs, _dismissals, complete, fail) => {
      const index = store.index("byTabRetentionOrder");
      const activeRange = IDBKeyRange.bound([tabId, "active", 0], [tabId, "active", Number.MAX_SAFE_INTEGER]);
      const settledRange = IDBKeyRange.bound([tabId, "settled", 0], [tabId, "settled", Number.MAX_SAFE_INTEGER]);
      const records: ChatSendDurableRecord[] = [];
      let rowsRead = 0;
      let settledRead = 0;
      let pending = 2;
      const finishOne = () => {
        pending -= 1;
        if (pending === 0) complete({ records: orderChatSendRecords(records), rowsRead });
      };
      const activeRequest = index.getAll(activeRange);
      activeRequest.onsuccess = () => {
        const loaded = activeRequest.result as unknown[];
        const active = loaded.filter(isDurableRecord);
        rowsRead += loaded.length;
        records.push(...active);
        finishOne();
      };
      activeRequest.onerror = () => fail(activeRequest.error ?? new Error("chat send active load failed"));
      const settledRequest = index.openCursor(settledRange, "prev");
      settledRequest.onsuccess = () => {
        const cursor = settledRequest.result;
        if (!cursor) {
          finishOne();
          return;
        }
        rowsRead += 1;
        if (isDurableRecord(cursor.value)) records.push(cursor.value);
        settledRead += 1;
        if (settledRead >= MAX_SETTLED_RECORDS) {
          finishOne();
          return;
        }
        cursor.continue();
      };
      settledRequest.onerror = () => fail(settledRequest.error ?? new Error("chat send settled load failed"));
    });
  }

  loadDismissals(tabId: string, identities: readonly ChatSendDismissalIdentity[]): Promise<ChatSendDismissalMarker[]> {
    return this.run("readonly", (_store, _tabs, dismissals, complete, fail) => {
      if (!dismissals) {
        fail(new Error("chat send dismissal store unavailable"));
        return;
      }
      const found = new Map<string, ChatSendDismissalMarker>();
      let pending = 0;
      const queued = new Set<string>();
      const collect = (key: string, request: IDBRequest<unknown>) => {
        if (queued.has(key)) return;
        queued.add(key);
        pending += 1;
        request.onsuccess = () => {
          const value = request.result;
          if (isChatSendDismissalMarker(value)) found.set(value.sendId, value);
          pending -= 1;
          if (pending === 0) complete([...found.values()]);
        };
        request.onerror = () => fail(request.error ?? new Error("chat send dismissal lookup failed"));
      };
      for (const identity of identities) {
        collect(JSON.stringify(["id", tabId, identity.id]), dismissals.get([tabId, identity.id]));
        if (identity.correlationId) collect(JSON.stringify(["correlation", tabId, identity.correlationId]),
          dismissals.index("byTabCorrelation").get([tabId, identity.correlationId]));
        if (identity.echoKey) collect(JSON.stringify(["echo", tabId, identity.echoKey]),
          dismissals.index("byTabEcho").get([tabId, identity.echoKey]));
        for (const key of chatSendDismissalMatchedRowKeys(identity)) {
          collect(JSON.stringify(["matched", tabId, key]), dismissals.index("byTabMatchedRow").get([tabId, key]));
        }
      }
      if (pending === 0) complete([]);
    }, false, true);
  }

  readDismissal(tabId: string, sendId: string): Promise<ChatSendDismissalMarker | undefined> {
    return this.run("readonly", (_store, _tabs, dismissals, complete, fail) => {
      if (!dismissals) {
        fail(new Error("chat send dismissal store unavailable"));
        return;
      }
      const request = dismissals.get([tabId, sendId]);
      request.onsuccess = () => complete(isChatSendDismissalMarker(request.result) ? request.result : undefined);
      request.onerror = () => fail(request.error ?? new Error("chat send dismissal read failed"));
    }, false, true);
  }

  read(tabId: string, sendId: string): Promise<ChatSendDurableRecord | undefined> {
    return this.run("readonly", (store, _tabs, _dismissals, complete) => {
      const request = store.get([tabId, sendId]);
      request.onsuccess = () => complete(isDurableRecord(request.result) ? request.result : undefined);
    });
  }

  write(tabId: string, previous: DeliverySend | undefined, next: DeliverySend): Promise<ChatSendRecordWrite> {
    return this.run("readwrite", (store, tabs, dismissals, complete, fail) => {
      const request = store.get([tabId, next.id]);
      request.onsuccess = () => {
        const prior = isDurableRecord(request.result) ? request.result : undefined;
        if (prior?.deleted) {
          if (!dismissals) {
            complete({ record: prior, changed: false });
            return;
          }
          const markerRead = dismissals.get([tabId, next.id]);
          markerRead.onsuccess = () => {
            if (!isChatSendDismissalMarker(markerRead.result)) {
              complete({ record: prior, changed: false });
              return;
            }
            const marker = encodeChatSendDismissalMarker(tabId, { ...next, dismissed: true },
              prior.revision + 1, prior.order);
            this.putDismissalMarkerIfChanged(dismissals, marker, (changed) => {
              complete({ record: prior, changed });
            }, fail);
          };
          markerRead.onerror = () => fail(markerRead.error ?? new Error("chat send dismissal read failed"));
          return;
        }
        const writeWithOrder = (order: number) => {
          const current = prior ? decodeChatSend(prior) ?? undefined : undefined;
          const merged = mergeChatSendRecord(current, previous, next);
          const revision = (prior?.revision ?? -1) + 1;
          const compact = merged.dismissed && chatSendRetention(merged) === "settled";
          const durable = compact
            ? encodeChatSendTombstone(tabId, next.id, revision, order)
            : encodeChatSend(tabId, merged, revision, order);
          const recordChanged = !prior || !sameDurableRecord(prior, durable);
          const finish = (markerChanged: boolean) => {
            const changed = recordChanged || markerChanged;
            const putRecord = recordChanged ? store.put(durable) : undefined;
            const afterRecord = () => {
              const compactIfNeeded = () => {
                if (!recordChanged || durable.retention !== "settled") {
                  complete({ record: recordChanged ? durable : prior!, changed });
                  return;
                }
                this.compactSettledOverflow(store, dismissals, tabId,
                  () => complete({ record: recordChanged ? durable : prior!, changed }), fail);
              };
              compactIfNeeded();
            };
            if (!putRecord) {
              afterRecord();
              return;
            }
            putRecord.onsuccess = afterRecord;
            putRecord.onerror = () => fail(putRecord.error ?? new Error("chat send record write failed"));
          };
          if (merged.dismissed) {
            if (!dismissals) {
              fail(new Error("chat send dismissal store unavailable"));
              return;
            }
            const marker = encodeChatSendDismissalMarker(tabId, merged, revision, order);
            this.putDismissalMarkerIfChanged(dismissals, marker, finish, fail);
            return;
          }
          finish(false);
        };
        if (prior?.order !== undefined) writeWithOrder(prior.order);
        else this.allocateOrder(tabs, tabId, undefined, undefined, writeWithOrder, fail);
      };
      request.onerror = () => fail(request.error ?? new Error("chat send record read failed"));
    }, true, true);
  }

  dismiss(tabId: string, send: DeliverySend): Promise<boolean> {
    return this.run("readwrite", (store, tabs, dismissals, complete, fail) => {
      if (!dismissals) {
        fail(new Error("chat send dismissal store unavailable"));
        return;
      }
      const request = store.get([tabId, send.id]);
      request.onsuccess = () => {
        const prior = isDurableRecord(request.result) ? request.result : undefined;
        const current = prior && !prior.deleted ? decodeChatSend(prior) ?? undefined : undefined;
        const mergedSend = { ...current, ...send, dismissed: true,
          echo: send.echo ?? current?.echo,
          matchedRow: send.matchedRow ?? current?.matchedRow ?? current?.status?.matched_row } as DeliverySend;
        const persistWithOrder = (order: number) => {
          const revision = (prior?.revision ?? -1) + 1;
          const active = chatSendRetention(mergedSend) === "active";
          const nextRecord = active
            ? encodeChatSend(tabId, mergedSend, revision, order)
            : encodeChatSendTombstone(tabId, send.id, revision, order);
          const recordChanged = !prior || !sameDurableRecord(prior, nextRecord);
          const marker = encodeChatSendDismissalMarker(tabId, mergedSend, revision, order);
          this.putDismissalMarkerIfChanged(dismissals, marker, (markerChanged) => {
            if (!recordChanged) {
              complete(markerChanged);
              return;
            }
            const put = store.put(nextRecord);
            put.onsuccess = () => complete(true);
            put.onerror = () => fail(put.error ?? new Error("chat send dismissal record write failed"));
          }, fail);
        };
        if (prior?.deleted) {
          const order = prior.order;
          const revision = prior.revision;
          this.putDismissalMarkerIfChanged(dismissals,
            encodeChatSendDismissalMarker(tabId, mergedSend, revision, order),
            (changed) => complete(changed), fail);
        } else if (prior?.order !== undefined) persistWithOrder(prior.order);
        else this.allocateOrder(tabs, tabId, undefined, undefined, persistWithOrder, fail);
      };
      request.onerror = () => fail(request.error ?? new Error("chat send dismissal evidence read failed"));
    }, true, true);
  }

  remove(tabId: string, sendId: string, _previous: DeliverySend | undefined): Promise<ChatSendRecordWrite> {
    return this.run("readwrite", (store, tabs, _dismissals, complete, fail) => {
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
        const writeTombstone = (order: number) => {
          const tombstone = encodeChatSendTombstone(tabId, sendId, (prior?.revision ?? -1) + 1, order);
          const put = store.put(tombstone);
          put.onsuccess = () => complete({ record: tombstone, changed: true });
          put.onerror = () => fail(put.error ?? new Error("chat send tombstone write failed"));
        };
        if (prior?.order !== undefined) writeTombstone(prior.order);
        else this.allocateOrder(tabs, tabId, undefined, undefined, writeTombstone, fail);
      };
      request.onerror = () => fail(request.error ?? new Error("chat send record read failed"));
    }, true, true);
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
    queue: (store: IDBObjectStore, tabs: IDBObjectStore | undefined, dismissals: IDBObjectStore | undefined,
      complete: (value: T) => void, fail: (error: Error) => void) => void,
    withTabState = false,
    withDismissals = false,
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
        const stores = [RECORDS, ...(withTabState ? [TAB_STATE] : []), ...(withDismissals ? [DISMISSALS] : [])];
        transaction = this.database.transaction(stores, mode);
        transaction.oncomplete = () => finish();
        transaction.onerror = () => finish(transaction.error ?? new Error("chat send transaction failed"));
        transaction.onabort = () => finish(transaction.error ?? new Error("chat send transaction aborted"));
        queue(transaction.objectStore(RECORDS), withTabState ? transaction.objectStore(TAB_STATE) : undefined,
          withDismissals ? transaction.objectStore(DISMISSALS) : undefined,
          (next) => { value = next; hasValue = true; }, (error) => {
          try { transaction.abort(); } catch { finish(error); }
        });
      } catch (error) {
        finish(error instanceof Error ? error : new Error(String(error)));
      }
    });
  }

  private compactSettledOverflow(
    store: IDBObjectStore,
    dismissals: IDBObjectStore | undefined,
    tabId: string,
    complete: () => void,
    fail: (error: Error) => void,
  ): void {
    const index = store.index("byTabRetentionOrder");
    const range = IDBKeyRange.bound([tabId, "settled", 0], [tabId, "settled", Number.MAX_SAFE_INTEGER]);
    const count = index.count(range);
    count.onsuccess = () => {
      if (count.result <= MAX_SETTLED_RECORDS) {
        complete();
        return;
      }
      let retained = 0;
      const cursorRequest = index.openCursor(range, "prev");
      cursorRequest.onsuccess = () => {
        const cursor = cursorRequest.result;
        if (!cursor) {
          complete();
          return;
        }
        const prior = cursor.value as ChatSendDurableRecord;
        if (!isDurableRecord(prior) || prior.retention !== "settled") {
          cursor.continue();
          return;
        }
        if (retained < MAX_SETTLED_RECORDS) {
          retained += 1;
          cursor.continue();
          return;
        }
        const send = decodeChatSend(prior);
        const tombstone = encodeChatSendTombstone(tabId, prior.sendId, prior.revision + 1, prior.order);
        const compact = () => {
          const update = cursor.update(tombstone);
          update.onsuccess = () => cursor.continue();
          update.onerror = () => fail(update.error ?? new Error("chat send settled compaction failed"));
        };
        if (!send?.dismissed || !dismissals) {
          compact();
          return;
        }
        const marker = encodeChatSendDismissalMarker(tabId, send, prior.revision + 1, prior.order);
        const markerRead = dismissals.get([tabId, prior.sendId]);
        markerRead.onsuccess = () => {
          const existing = isChatSendDismissalMarker(markerRead.result) ? markerRead.result : undefined;
          const markerPut = dismissals.put(mergeChatSendDismissalMarker(existing, marker));
          markerPut.onsuccess = compact;
          markerPut.onerror = () => fail(markerPut.error ?? new Error("chat send marker compaction failed"));
        };
        markerRead.onerror = () => fail(markerRead.error ?? new Error("chat send marker read failed"));
      };
      cursorRequest.onerror = () => fail(cursorRequest.error ?? new Error("chat send settled scan failed"));
    };
    count.onerror = () => fail(count.error ?? new Error("chat send settled count failed"));
  }

  private putDismissalMarkerIfChanged(
    dismissals: IDBObjectStore,
    next: ChatSendDismissalMarker,
    complete: (changed: boolean) => void,
    fail: (error: Error) => void,
  ): void {
    const request = dismissals.get([next.tabId, next.sendId]);
    request.onsuccess = () => {
      const current = isChatSendDismissalMarker(request.result) ? request.result : undefined;
      const merged = mergeChatSendDismissalMarker(current, next);
      if (current && sameChatSendDismissalMarker(current, merged)) {
        complete(false);
        return;
      }
      const put = dismissals.put(merged);
      put.onsuccess = () => complete(true);
      put.onerror = () => fail(put.error ?? new Error("chat send dismissal marker write failed"));
    };
    request.onerror = () => fail(request.error ?? new Error("chat send dismissal marker read failed"));
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

export function openChatSendDatabase(): Promise<ChatSendRecordDatabase | null> {
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
    request.onupgradeneeded = (event) => {
      if (event.oldVersion !== 0) {
        request.transaction?.abort();
        publishNotice({
          kind: "toast", id: "chat-send-storage-version", severity: "Error",
          title: "Unsupported saved chat format",
          detail: `Chat storage version ${event.oldVersion} is no longer supported. Existing data was left unchanged. Clear this site's chat storage to start fresh.`,
        });
        finish(null);
        return;
      }
      const database = request.result;
      const records = database.createObjectStore(RECORDS, { keyPath: ["tabId", "sendId"] });
      records.createIndex("byTab", "tabId", { unique: false });
      records.createIndex("byTabRetentionOrder", ["tabId", "retention", "order"], { unique: false });
      database.createObjectStore(TAB_STATE, { keyPath: "tabId" });
      const dismissals = database.createObjectStore(DISMISSALS, { keyPath: ["tabId", "sendId"] });
      dismissals.createIndex("byTabCorrelation", ["tabId", "correlationId"], { unique: false });
      dismissals.createIndex("byTabMatchedRow", ["tabId", "matchedRowKey"], { unique: false });
      dismissals.createIndex("byTabEcho", ["tabId", "echoKey"], { unique: false });
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
