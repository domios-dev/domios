import { apiHost, relayTarget } from "./webApi";
import type { SavedPageSelection } from "../store/multiplexer/pageSelection";

function storageKeys(): { workspace: string; tab: string } {
  let origin = window.location?.origin ?? "local";
  try { origin = apiHost(); } catch {}
  const hostScope = origin === window.location?.origin ? "" : `:${origin}`;
  const peer = relayTarget()?.id;
  const scope = `${hostScope}${peer ? `:${peer}` : ""}`;
  return { workspace: `codeterm.sidebar.selected-workspace${scope}`, tab: `codeterm.sidebar.selected-tab${scope}` };
}

function read(storage: Storage, keys: { workspace: string; tab: string }): SavedPageSelection | null {
  const workspaceId = storage.getItem(keys.workspace);
  return workspaceId ? { workspaceId, tabId: storage.getItem(keys.tab) } : null;
}

function write(storage: Storage, keys: { workspace: string; tab: string }, selection: SavedPageSelection): void {
  storage.setItem(keys.workspace, selection.workspaceId);
  if (selection.tabId) storage.setItem(keys.tab, selection.tabId);
  else storage.removeItem(keys.tab);
}

/** This window's own selection first; a fresh window on the same device starts where that device last was. */
export function loadClientPageSelection(): SavedPageSelection | null {
  const keys = storageKeys();
  for (const storage of [() => window.sessionStorage, () => window.localStorage]) {
    try {
      const saved = read(storage(), keys);
      if (saved) return saved;
    } catch {}
  }
  return null;
}

export function saveClientPageSelection(selection: SavedPageSelection): void {
  const keys = storageKeys();
  for (const storage of [() => window.sessionStorage, () => window.localStorage]) {
    try { write(storage(), keys, selection); } catch {}
  }
}
