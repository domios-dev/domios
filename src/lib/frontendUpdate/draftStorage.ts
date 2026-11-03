import { frontendCacheScope } from "../frontendCacheRecords";

export interface DraftStorage {
  read(): Promise<Map<string, string>>;
  write(scope: string, value: string | null): Promise<void>;
}

export function createDraftStorage(): DraftStorage {
  let database: Promise<IDBDatabase> | undefined;
  const open = () => database ??= new Promise<IDBDatabase>((resolve, reject) => {
    if (typeof indexedDB === "undefined") { reject(new Error("Draft storage is unavailable")); return; }
    const request = indexedDB.open("codeterm-composer-drafts", 1);
    let blocked = false;
    request.onupgradeneeded = () => {
      const store = request.result.createObjectStore("drafts", { keyPath: ["client", "scope"] });
      store.createIndex("client", "client");
    };
    request.onerror = () => reject(request.error ?? new Error("Draft storage could not open"));
    request.onblocked = () => { blocked = true; reject(new Error("Draft storage is blocked by another window")); };
    request.onsuccess = () => {
      if (blocked) request.result.close();
      else {
        request.result.onversionchange = () => { request.result.close(); database = undefined; };
        resolve(request.result);
      }
    };
  }).catch(error => { database = undefined; throw error; });
  const clientScope = () => {
    const client = frontendCacheScope("composer-drafts");
    if (!client) throw new Error("Draft storage client scope is unavailable");
    return client;
  };
  return {
    async read() {
      const client = clientScope();
      const db = await open();
      return new Promise((resolve, reject) => {
        const transaction = db.transaction("drafts", "readonly");
        const request = transaction.objectStore("drafts").index("client").getAll(client);
        transaction.oncomplete = () => resolve(new Map((request.result as { scope: string; value: string }[]).map(record => [record.scope, record.value])));
        transaction.onerror = transaction.onabort = () => reject(transaction.error ?? new Error("Draft restore failed"));
      });
    },
    async write(scope, value) {
      const client = clientScope();
      const db = await open();
      await new Promise<void>((resolve, reject) => {
        const transaction = db.transaction("drafts", "readwrite");
        if (value === null) transaction.objectStore("drafts").delete([client, scope]);
        else transaction.objectStore("drafts").put({ client, scope, value });
        transaction.oncomplete = () => resolve();
        transaction.onerror = transaction.onabort = () => reject(transaction.error ?? new Error("Draft save failed"));
      });
    },
  };
}
