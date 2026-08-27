/**
 * Client-side cache of the last authoritative `mux_get_state` snapshot, used
 * to paint the sidebar/workspaces/tabs instantly on boot before the daemon
 * round-trip resolves (web-only — native Tauri always has the daemon local).
 * Keyed by daemon host + auth token + relay peer so a cache can never leak
 * across daemons, accounts, or peer contexts; dropped on any schema or build
 * mismatch, and degrades to no-op if storage is unavailable (private
 * browsing).
 */
import { buildHash } from "./buildStamp";
import type { MultiplexerState } from "./types";
import { IS_WEB, apiHost, currentToken, relayTarget } from "./webApi";

const SCHEMA_VERSION = 1;
const LOCAL_STORAGE_SIZE_LIMIT = 200 * 1024;
const DB_NAME = "codeterm-state-cache";
const STORE_NAME = "snapshot";

interface CachedPayload {
  schemaVersion: number;
  buildHash: string;
  savedAt: number;
  state: MultiplexerState;
}

// deterministic per host+identity pair.
function hashKey(input: string): string {
  let hash = 5381;
  for (let i = 0; i < input.length; i++) {
    hash = (hash * 33) ^ input.charCodeAt(i);
  }
  return (hash >>> 0).toString(36);
}

function cacheKey(): string | null {
  if (!IS_WEB) return null;
  try {
    return `codeterm:state-cache:${hashKey(`${apiHost()}:${currentToken() ?? ""}:${relayTarget()?.id ?? "self"}`)}`;
  } catch {
    return null;
  }
}

function isValid(payload: CachedPayload | null | undefined): payload is CachedPayload {
  return (
    !!payload &&
    payload.schemaVersion === SCHEMA_VERSION &&
    payload.buildHash === buildHash()
  );
}

function readLocalStorage(key: string): CachedPayload | null {
  try {
    const raw = localStorage.getItem(key);
    return raw ? (JSON.parse(raw) as CachedPayload) : null;
  } catch {
    return null;
  }
}

function writeLocalStorage(key: string, payload: CachedPayload): boolean {
  try {
    localStorage.setItem(key, JSON.stringify(payload));
    return true;
  } catch {
    return false;
  }
}

function removeLocalStorage(key: string): void {
  try {
    localStorage.removeItem(key);
  } catch {
  }
}

function openIndexedDb(): Promise<IDBDatabase | null> {
  return new Promise((resolve) => {
    if (typeof indexedDB === "undefined") {
      resolve(null);
      return;
    }
    let req: IDBOpenDBRequest;
    try {
      req = indexedDB.open(DB_NAME, 1);
    } catch {
      resolve(null);
      return;
    }
    req.onupgradeneeded = () => {
      const db = req.result;
      if (!db.objectStoreNames.contains(STORE_NAME)) db.createObjectStore(STORE_NAME);
    };
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => resolve(null);
  });
}

async function readIndexedDb(key: string): Promise<CachedPayload | null> {
  const db = await openIndexedDb();
  if (!db) return null;
  return new Promise((resolve) => {
    try {
      const tx = db.transaction(STORE_NAME, "readonly");
      const req = tx.objectStore(STORE_NAME).get(key);
      req.onsuccess = () => resolve((req.result as CachedPayload | undefined) ?? null);
      req.onerror = () => resolve(null);
    } catch {
      resolve(null);
    } finally {
      db.close();
    }
  });
}

async function writeIndexedDb(key: string, payload: CachedPayload): Promise<void> {
  const db = await openIndexedDb();
  if (!db) return;
  return new Promise((resolve) => {
    try {
      const tx = db.transaction(STORE_NAME, "readwrite");
      tx.objectStore(STORE_NAME).put(payload, key);
      tx.oncomplete = () => resolve();
      tx.onerror = () => resolve();
    } catch {
      resolve();
    } finally {
      db.close();
    }
  });
}

function removeIndexedDb(key: string): void {
  openIndexedDb().then((db) => {
    if (!db) return;
    try {
      const tx = db.transaction(STORE_NAME, "readwrite");
      tx.objectStore(STORE_NAME).delete(key);
      tx.oncomplete = () => db.close();
      tx.onerror = () => db.close();
    } catch {
      db.close();
    }
  });
}

/**
 * Synchronous, localStorage-only load — safe to call from React's lazy
 * `useReducer` initializer so the very first paint already reflects the
 * cache (no flash of an empty shell).
 */
export function loadCachedStateSync(): MultiplexerState | null {
  const key = cacheKey();
  if (!key) return null;
  const payload = readLocalStorage(key);
  if (!isValid(payload)) {
    if (payload) removeLocalStorage(key);
    return null;
  }
  return payload.state;
}

/** IndexedDB fallback for snapshots too large to have fit in localStorage. */
export async function loadCachedStateAsync(): Promise<MultiplexerState | null> {
  const key = cacheKey();
  if (!key) return null;
  const payload = await readIndexedDb(key);
  if (!isValid(payload)) {
    if (payload) removeIndexedDb(key);
    return null;
  }
  return payload.state;
}

/** Best-effort persist; picks localStorage when small enough, else IndexedDB. */
export function saveStateSnapshot(state: MultiplexerState): void {
  const key = cacheKey();
  if (!key) return;
  const payload: CachedPayload = {
    schemaVersion: SCHEMA_VERSION,
    buildHash: buildHash(),
    savedAt: Date.now(),
    state,
  };
  const json = JSON.stringify(payload);
  if (json.length <= LOCAL_STORAGE_SIZE_LIMIT && writeLocalStorage(key, payload)) {
    removeIndexedDb(key);
    return;
  }
  removeLocalStorage(key);
  writeIndexedDb(key, payload).catch(() => {});
}

export function clearStateSnapshot(): void {
  const key = cacheKey();
  if (!key) return;
  removeLocalStorage(key);
  removeIndexedDb(key);
}

/**
 * Blocks daemon-mutating calls until the first live reconcile lands, so a
 * cache-painted UI can never act on a stale/nonexistent target.
 */
export interface WriteGate {
  canWrite(): boolean;
  lock(): void;
  unlock(): void;
}

export function createWriteGate(locked = false): WriteGate {
  let isLocked = locked;
  return {
    canWrite: () => !isLocked,
    lock: () => {
      isLocked = true;
    },
    unlock: () => {
      isLocked = false;
    },
  };
}
