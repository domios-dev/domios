import { DEV_BUILD } from "../devBuild";
import { frontendCacheScope } from "../frontendCacheRecords";

export interface DraftStorage {
  read(): Promise<Map<string, string>>;
  write(scope: string, value: string | null): Promise<void>;
}

interface DatabaseEntry {
  promise: Promise<IDBDatabase>;
  db?: IDBDatabase;
  closing: boolean;
}

export function isClosingDatabaseError(error: unknown): boolean {
  if (typeof error === "string") return /connection is closing/i.test(error);
  if (!error || typeof error !== "object") return false;
  const { name, message } = error as { name?: unknown; message?: unknown };
  return name === "InvalidStateError" || (typeof message === "string" && /connection is closing/i.test(message));
}

export function createDraftStorage(idb?: Pick<IDBFactory, "open">): DraftStorage {
  let current: DatabaseEntry | undefined;

  const drop = (entry: DatabaseEntry, close: boolean) => {
    entry.closing = true;
    if (current === entry) current = undefined;
    const handle = entry.db;
    entry.db = undefined;
    if (close && handle) {
      try { handle.close(); } catch {}
    }
  };

  const open = async (): Promise<{ entry: DatabaseEntry; db: IDBDatabase }> => {
    if (current?.closing) current = undefined;
    let entry = current;
    if (!entry) {
      const created: DatabaseEntry = {
        closing: false,
        promise: undefined as unknown as Promise<IDBDatabase>,
      };
      created.promise = new Promise<IDBDatabase>((resolve, reject) => {
        const factory = idb ?? (typeof indexedDB === "undefined" ? undefined : indexedDB);
        if (!factory) { reject(new Error("Draft storage is unavailable")); return; }
        const request = factory.open("codeterm-composer-drafts", 1);
        let settled = false;
        let blocked = false;
        request.onupgradeneeded = () => {
          const db = request.result;
          if (!db.objectStoreNames || !db.objectStoreNames.contains("drafts")) {
            const store = db.createObjectStore("drafts", { keyPath: ["client", "scope"] });
            store.createIndex("client", "client");
          }
        };
        request.onerror = () => {
          if (settled) return;
          settled = true;
          reject(request.error ?? new Error("Draft storage could not open"));
        };
        request.onblocked = () => {
          blocked = true;
          if (settled) return;
          settled = true;
          reject(new Error("Draft storage is blocked by another window"));
        };
        request.onsuccess = () => {
          const db = request.result;
          if (blocked || created.closing) {
            try { db.close(); } catch {}
            if (!settled) {
              settled = true;
              reject(new Error("The database connection is closing."));
            }
            return;
          }
          created.db = db;
          db.onversionchange = () => drop(created, true);
          db.onclose = () => drop(created, true);
          settled = true;
          resolve(db);
        };
      }).catch(error => {
        drop(created, true);
        throw error;
      });
      current = created;
      entry = created;
    }
    const db = await entry.promise;
    if (entry.closing) {
      drop(entry, true);
      throw new Error("The database connection is closing.");
    }
    return { entry, db };
  };

  const run = async <T>(operation: (db: IDBDatabase) => Promise<T>): Promise<T> => {
    for (let attempt = 0; ; attempt += 1) {
      let entry: DatabaseEntry | undefined;
      try {
        const opened = await open();
        entry = opened.entry;
        return await operation(opened.db);
      } catch (error) {
        const closing = isClosingDatabaseError(error);
        if (entry && closing) drop(entry, true);
        if (attempt === 0 && closing) {
          if (DEV_BUILD) console.debug("[draft-storage] Reopening closing IndexedDB connection", error);
          continue;
        }
        throw error;
      }
    }
  };

  const clientScope = () => {
    const client = frontendCacheScope("composer-drafts");
    if (!client) throw new Error("Draft storage client scope is unavailable");
    return client;
  };

  return {
    async read() {
      const client = clientScope();
      return run(db => new Promise((resolve, reject) => {
        const transaction = db.transaction("drafts", "readonly");
        const request = transaction.objectStore("drafts").index("client").getAll(client);
        transaction.oncomplete = () => resolve(new Map((request.result as { scope: string; value: string }[]).map(record => [record.scope, record.value])));
        transaction.onerror = transaction.onabort = () => reject(transaction.error ?? new Error("Draft restore failed"));
      }));
    },
    async write(scope, value) {
      const client = clientScope();
      await run(db => new Promise<void>((resolve, reject) => {
        const transaction = db.transaction("drafts", "readwrite");
        if (value === null) transaction.objectStore("drafts").delete([client, scope]);
        else transaction.objectStore("drafts").put({ client, scope, value });
        transaction.oncomplete = () => resolve();
        transaction.onerror = transaction.onabort = () => reject(transaction.error ?? new Error("Draft save failed"));
      }));
    },
  };
}
