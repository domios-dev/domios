const UNKNOWN_LOCAL_NAME = "Domios";
const MESH_PEERS_LOADED = "codeterm-mesh-peers-loaded";
const STORAGE_KEY = "codeterm-local-instance-name";

function readPersistedName(): string | null {
  try {
    return localStorage.getItem(STORAGE_KEY)?.trim() || null;
  } catch {
    return null;
  }
}

let knownName: string | null = readPersistedName();

/** Keeps the last name the local daemon reported, so a failed fetch, a remount or a reload never reverts the label. */
export function rememberLocalInstanceName(name: string | null | undefined): string {
  const trimmed = name?.trim();
  if (trimmed) {
    knownName = trimmed;
    try {
      localStorage.setItem(STORAGE_KEY, trimmed);
    } catch {
      // Persistence is a first-paint convenience; the in-memory name still holds.
    }
  }
  return localInstanceLabel();
}

export function hasLocalInstanceName(): boolean {
  return knownName !== null;
}

export function localInstanceLabel(): string {
  return knownName ?? UNKNOWN_LOCAL_NAME;
}

export function notifyMeshPeersLoaded(): void {
  window.dispatchEvent(new Event(MESH_PEERS_LOADED));
}

export function onMeshPeersLoaded(handler: () => void): () => void {
  window.addEventListener(MESH_PEERS_LOADED, handler);
  return () => window.removeEventListener(MESH_PEERS_LOADED, handler);
}

export function __resetLocalInstanceNameForTest(): void {
  knownName = null;
  try {
    localStorage.removeItem(STORAGE_KEY);
  } catch {
    // Nothing persisted to clear.
  }
}

export function __reloadLocalInstanceNameForTest(): void {
  knownName = readPersistedName();
}
