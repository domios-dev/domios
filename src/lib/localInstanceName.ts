const UNKNOWN_LOCAL_NAME = "Domios";
const MESH_PEERS_LOADED = "codeterm-mesh-peers-loaded";

let knownName: string | null = null;

/** Keeps the last name the local daemon reported, so a failed fetch or a remount never reverts the label. */
export function rememberLocalInstanceName(name: string | null | undefined): string {
  const trimmed = name?.trim();
  if (trimmed) knownName = trimmed;
  return localInstanceLabel();
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
}
