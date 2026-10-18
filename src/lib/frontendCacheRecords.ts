import { apiHost, currentToken, relayTarget } from "./webApi";

const DATABASE = "codeterm-frontend-cache";
const DATABASE_VERSION = 1;
const RECORDS = "records";
const MANIFESTS = "manifests";

export interface FrontendCacheRecord {
  kind: string;
  id: string;
  value: unknown;
}

export interface FrontendCacheManifest {
  scope: string;
  revision: number;
  recordCount: number;
  metadata: Record<string, unknown>;
}

export interface FrontendCacheSnapshot {
  manifest: FrontendCacheManifest;
  records: FrontendCacheRecord[];
}

export interface FrontendCacheChanges {
  upserts: FrontendCacheRecord[];
  deletes: Array<{ kind: string; id: string }>;
}

export type FrontendCacheCommit = "committed" | "conflict";

export interface FrontendCacheStorage {
  read(scope: string): Promise<FrontendCacheSnapshot | null>;
  commit(
    scope: string,
    expectedRevision: number,
    manifest: FrontendCacheManifest,
    changes: FrontendCacheChanges,
  ): Promise<FrontendCacheCommit>;
  clear(scope: string, expectedRevision?: number): Promise<boolean>;
}

export interface FrontendCacheWriteResult {
  recordWrites: number;
  recordDeletes: number;
  manifestWrites: number;
  refused: "stale-generation" | null;
}

function recordKey(kind: string, id: string): string {
  return `${kind}\u0000${id}`;
}

export function sameCacheValue(left: unknown, right: unknown): boolean {
  if (Object.is(left, right)) return true;
  if (!left || !right || typeof left !== "object" || typeof right !== "object") return false;
  if (Array.isArray(left) || Array.isArray(right)) {
    return Array.isArray(left) && Array.isArray(right) && left.length === right.length &&
      left.every((value, index) => sameCacheValue(value, right[index]));
  }
  const leftRecord = left as Record<string, unknown>;
  const rightRecord = right as Record<string, unknown>;
  const leftKeys = Object.keys(leftRecord);
  const rightKeys = Object.keys(rightRecord);
  return leftKeys.length === rightKeys.length &&
    leftKeys.every((key) => Object.prototype.hasOwnProperty.call(rightRecord, key) && sameCacheValue(leftRecord[key], rightRecord[key]));
}

function uniqueRecordKeys(records: FrontendCacheRecord[]): boolean {
  const keys = new Set<string>();
  for (const record of records) {
    const key = recordKey(record.kind, record.id);
    if (keys.has(key)) return false;
    keys.add(key);
  }
  return true;
}

export function diffFrontendCacheRecords(
  previous: FrontendCacheRecord[],
  next: FrontendCacheRecord[],
): FrontendCacheChanges {
  const previousByKey = new Map(previous.map((record) => [recordKey(record.kind, record.id), record]));
  const nextByKey = new Map(next.map((record) => [recordKey(record.kind, record.id), record]));
  const upserts: FrontendCacheRecord[] = [];
  const deletes: Array<{ kind: string; id: string }> = [];
  for (const [key, record] of nextByKey) {
    const before = previousByKey.get(key);
    if (!before || !sameCacheValue(before.value, record.value)) upserts.push(record);
  }
  for (const [key, record] of previousByKey) {
    if (!nextByKey.has(key)) deletes.push({ kind: record.kind, id: record.id });
  }
  return { upserts, deletes };
}

function validSnapshot(snapshot: FrontendCacheSnapshot | null, scope: string): snapshot is FrontendCacheSnapshot {
  if (!snapshot || typeof snapshot !== "object" || !snapshot.manifest || typeof snapshot.manifest !== "object" ||
    !Array.isArray(snapshot.records) || snapshot.manifest.scope !== scope ||
    !Number.isInteger(snapshot.manifest.revision) || snapshot.manifest.revision <= 0 ||
    snapshot.manifest.recordCount !== snapshot.records.length || !snapshot.manifest.metadata ||
    typeof snapshot.manifest.metadata !== "object") return false;
  if (!snapshot.records.every((record) => record && typeof record === "object" &&
    typeof record.kind === "string" && typeof record.id === "string")) return false;
  return uniqueRecordKeys(snapshot.records);
}

function generationOf(manifest: FrontendCacheManifest | null): number | null {
  const value = manifest?.metadata.generation;
  return typeof value === "number" ? value : null;
}

function revisionOf(snapshot: unknown): number | undefined {
  if (!snapshot || typeof snapshot !== "object") return undefined;
  const revision = (snapshot as { manifest?: { revision?: unknown } }).manifest?.revision;
  return typeof revision === "number" && Number.isInteger(revision) ? revision : undefined;
}

export class FrontendCacheRepository {
  private readonly pending = new Map<string, Promise<unknown>>();
  private readonly current = new Map<string, FrontendCacheSnapshot | null>();

  constructor(private readonly storage: FrontendCacheStorage) {}

  read(scope: string): Promise<FrontendCacheSnapshot | null> {
    return this.enqueue(scope, async () => {
      const snapshot = await this.storage.read(scope);
      if (!validSnapshot(snapshot, scope)) {
        let cleared = false;
        const revision = revisionOf(snapshot);
        if (revision !== undefined) cleared = await this.storage.clear(scope, revision);
        if (!cleared) {
          const latest = await this.storage.read(scope);
          if (validSnapshot(latest, scope)) {
            this.current.set(scope, latest);
            return latest;
          }
        }
        this.current.set(scope, null);
        return null;
      }
      this.current.set(scope, snapshot);
      return snapshot;
    });
  }

  write(
    scope: string,
    records: FrontendCacheRecord[],
    metadata: Record<string, unknown>,
  ): Promise<FrontendCacheWriteResult> {
    if (!uniqueRecordKeys(records)) return Promise.reject(new Error("Duplicate frontend cache record key"));
    return this.enqueue(scope, async () => {
      let previous = this.current.has(scope) ? this.current.get(scope)! : await this.storage.read(scope);
      if (!validSnapshot(previous, scope)) {
        let cleared = false;
        const revision = revisionOf(previous);
        if (revision !== undefined) cleared = await this.storage.clear(scope, revision);
        if (!cleared) {
          const latest = await this.storage.read(scope);
          previous = validSnapshot(latest, scope) ? latest : null;
        } else {
          previous = null;
        }
      }
      const incomingGeneration = generationOf({ scope, revision: 0, recordCount: 0, metadata });
      const previousGeneration = generationOf(previous?.manifest ?? null);
      if (incomingGeneration !== null && previousGeneration !== null && incomingGeneration < previousGeneration) {
        return { recordWrites: 0, recordDeletes: 0, manifestWrites: 0, refused: "stale-generation" };
      }

      const changes = diffFrontendCacheRecords(previous?.records ?? [], records);
      const sameMetadata = sameCacheValue(previous?.manifest.metadata ?? null, metadata);
      if (changes.upserts.length === 0 && changes.deletes.length === 0 && sameMetadata) {
        return { recordWrites: 0, recordDeletes: 0, manifestWrites: 0, refused: null };
      }

      const expectedRevision = previous?.manifest.revision ?? 0;
      const manifest: FrontendCacheManifest = {
        scope,
        revision: expectedRevision + 1,
        recordCount: records.length,
        metadata,
      };
      const commit = await this.storage.commit(scope, expectedRevision, manifest, changes);
      if (commit === "conflict") {
        const latest = await this.storage.read(scope);
        const validLatest = validSnapshot(latest, scope) ? latest : null;
        if (latest && !validLatest) {
          const revision = revisionOf(latest);
          if (revision !== undefined) await this.storage.clear(scope, revision);
        }
        const latestGeneration = generationOf(validLatest?.manifest ?? null);
        const canSupersede = incomingGeneration !== null && latestGeneration !== null && incomingGeneration > latestGeneration;
        this.current.set(scope, validLatest);
        if (!canSupersede) return { recordWrites: 0, recordDeletes: 0, manifestWrites: 0, refused: "stale-generation" };
        const rebased = diffFrontendCacheRecords(validLatest?.records ?? [], records);
        const rebasedManifest: FrontendCacheManifest = {
          scope,
          revision: (validLatest?.manifest.revision ?? 0) + 1,
          recordCount: records.length,
          metadata,
        };
        const retry = await this.storage.commit(scope, validLatest?.manifest.revision ?? 0, rebasedManifest, rebased);
        if (retry === "conflict") return { recordWrites: 0, recordDeletes: 0, manifestWrites: 0, refused: "stale-generation" };
        this.current.set(scope, { manifest: rebasedManifest, records });
        return { recordWrites: rebased.upserts.length, recordDeletes: rebased.deletes.length, manifestWrites: 1, refused: null };
      }

      this.current.set(scope, { manifest, records });
      return {
        recordWrites: changes.upserts.length,
        recordDeletes: changes.deletes.length,
        manifestWrites: 1,
        refused: null,
      };
    });
  }

  clear(scope: string, expectedRevision?: number): Promise<boolean> {
    return this.enqueue(scope, async () => {
      const cleared = await this.storage.clear(scope, expectedRevision);
      if (cleared) this.current.set(scope, null);
      else this.current.delete(scope);
      return cleared;
    });
  }

  reset(): void {
    this.current.clear();
    this.pending.clear();
  }

  private enqueue<T>(scope: string, operation: () => Promise<T>): Promise<T> {
    const previous = this.pending.get(scope) ?? Promise.resolve();
    const next = previous.then(operation, operation);
    this.pending.set(scope, next.catch(() => undefined));
    return next;
  }
}

class IndexedDbFrontendCacheStorage implements FrontendCacheStorage {
  private open(): Promise<IDBDatabase | null> {
    return new Promise((resolve) => {
      if (typeof indexedDB === "undefined") {
        resolve(null);
        return;
      }
      let request: IDBOpenDBRequest;
      try {
        request = indexedDB.open(DATABASE, DATABASE_VERSION);
      } catch {
        resolve(null);
        return;
      }
      let settled = false;
      request.onupgradeneeded = () => {
        const db = request.result;
        if (!db.objectStoreNames.contains(RECORDS)) {
          const records = db.createObjectStore(RECORDS, { keyPath: ["scope", "kind", "id"] });
          records.createIndex("scope", "scope", { unique: false });
        }
        if (!db.objectStoreNames.contains(MANIFESTS)) db.createObjectStore(MANIFESTS, { keyPath: "scope" });
      };
      request.onblocked = () => {
        if (!settled) {
          settled = true;
          resolve(null);
        }
      };
      request.onerror = () => {
        if (!settled) {
          settled = true;
          resolve(null);
        }
      };
      request.onsuccess = () => {
        if (settled) request.result.close();
        else {
          settled = true;
          resolve(request.result);
        }
      };
    });
  }

  async read(scope: string): Promise<FrontendCacheSnapshot | null> {
    const db = await this.open();
    if (!db) return null;
    return new Promise((resolve, reject) => {
      let manifest: FrontendCacheManifest | undefined;
      let records: FrontendCacheRecord[] = [];
      try {
        const tx = db.transaction([RECORDS, MANIFESTS], "readonly");
        const manifests = tx.objectStore(MANIFESTS);
        const rows = tx.objectStore(RECORDS).index("scope");
        const manifestRequest = manifests.get(scope);
        const recordsRequest = rows.getAll(IDBKeyRange.only(scope));
        manifestRequest.onsuccess = () => { manifest = manifestRequest.result as FrontendCacheManifest | undefined; };
        recordsRequest.onsuccess = () => {
          records = (recordsRequest.result as Array<{ kind: string; id: string; value: unknown }>).map(({ kind, id, value }) => ({ kind, id, value }));
        };
        tx.oncomplete = () => {
          db.close();
          if (manifest) resolve({ manifest, records });
          else if (records.length > 0) resolve({
            manifest: { scope, revision: 0, recordCount: 0, metadata: {} },
            records,
          });
          else resolve(null);
        };
        tx.onerror = () => {
          db.close();
          reject(tx.error ?? new Error("IndexedDB read failed"));
        };
        tx.onabort = () => {
          db.close();
          reject(tx.error ?? new Error("IndexedDB read aborted"));
        };
      } catch (error) {
        db.close();
        reject(error);
      }
    });
  }

  async commit(
    scope: string,
    expectedRevision: number,
    manifest: FrontendCacheManifest,
    changes: FrontendCacheChanges,
  ): Promise<FrontendCacheCommit> {
    const db = await this.open();
    if (!db) throw new Error("IndexedDB unavailable");
    return new Promise((resolve, reject) => {
      let conflict = false;
      try {
        const tx = db.transaction([RECORDS, MANIFESTS], "readwrite");
        const records = tx.objectStore(RECORDS);
        const manifests = tx.objectStore(MANIFESTS);
        const currentRequest = manifests.get(scope);
        currentRequest.onsuccess = () => {
          const current = currentRequest.result as FrontendCacheManifest | undefined;
          if ((current?.revision ?? 0) !== expectedRevision) {
            conflict = true;
            tx.abort();
            return;
          }
          for (const row of changes.deletes) records.delete([scope, row.kind, row.id]);
          for (const row of changes.upserts) records.put({ scope, ...row });
          manifests.put(manifest);
        };
        tx.oncomplete = () => {
          db.close();
          resolve("committed");
        };
        tx.onerror = () => {
          db.close();
          reject(tx.error ?? new Error("IndexedDB write failed"));
        };
        tx.onabort = () => {
          db.close();
          if (conflict) resolve("conflict");
          else reject(tx.error ?? new Error("IndexedDB write aborted"));
        };
      } catch (error) {
        db.close();
        reject(error);
      }
    });
  }

  async clear(scope: string, expectedRevision?: number): Promise<boolean> {
    const db = await this.open();
    if (!db) return false;
    return new Promise((resolve, reject) => {
      try {
        const tx = db.transaction([RECORDS, MANIFESTS], "readwrite");
        const records = tx.objectStore(RECORDS);
        const keys = records.index("scope").getAllKeys(IDBKeyRange.only(scope));
        const manifestRequest = tx.objectStore(MANIFESTS).get(scope);
        let recordKeys: IDBValidKey[] | null = null;
        let current: FrontendCacheManifest | undefined;
        let manifestLoaded = false;
        let applied = false;
        let cleared = false;
        const apply = () => {
          if (applied || recordKeys === null || !manifestLoaded) return;
          applied = true;
          if (expectedRevision !== undefined && (current?.revision ?? 0) !== expectedRevision) return;
          for (const key of recordKeys) records.delete(key);
          tx.objectStore(MANIFESTS).delete(scope);
          cleared = true;
        };
        keys.onsuccess = () => { recordKeys = keys.result; apply(); };
        manifestRequest.onsuccess = () => {
          current = manifestRequest.result as FrontendCacheManifest | undefined;
          manifestLoaded = true;
          apply();
        };
        tx.oncomplete = () => { db.close(); resolve(cleared); };
        tx.onerror = () => { db.close(); reject(tx.error ?? new Error("IndexedDB clear failed")); };
        tx.onabort = () => { db.close(); reject(tx.error ?? new Error("IndexedDB clear aborted")); };
      } catch (error) {
        db.close();
        reject(error);
      }
    });
  }
}

const indexedDbStorage = new IndexedDbFrontendCacheStorage();
let activeStorage: FrontendCacheStorage = indexedDbStorage;
const storageProxy: FrontendCacheStorage = {
  read: (...args) => activeStorage.read(...args),
  commit: (...args) => activeStorage.commit(...args),
  clear: (...args) => activeStorage.clear(...args),
};

export const frontendCacheRepository = new FrontendCacheRepository(storageProxy);

export function setFrontendCacheStorageForTests(storage: FrontendCacheStorage | null): void {
  activeStorage = storage ?? indexedDbStorage;
  frontendCacheRepository.reset();
}

export function frontendCacheScope(domain: string): string | null {
  try {
    const isWeb = typeof window !== "undefined" && !("__TAURI_INTERNALS__" in window);
    const host = isWeb
      ? apiHost()
      : typeof window === "undefined" ? "native" : window.location.origin ?? `${window.location.protocol}//${window.location.host}`;
    const account = isWeb ? currentToken() ?? "" : "native-account";
    const input = `${host}:${account}:${relayTarget()?.id ?? "self"}`;
    let hash = 5381;
    for (let index = 0; index < input.length; index++) hash = (hash * 33) ^ input.charCodeAt(index);
    return `${domain}:${(hash >>> 0).toString(36)}`;
  } catch {
    return null;
  }
}
