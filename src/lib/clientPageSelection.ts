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

export function loadClientPageSelection(): SavedPageSelection | null {
  try {
    const keys = storageKeys();
    const workspaceId = window.localStorage.getItem(keys.workspace);
    if (workspaceId) return { workspaceId, tabId: window.localStorage.getItem(keys.tab) };
  } catch {}
  return null;
}

export function saveClientPageSelection(selection: SavedPageSelection): void {
  try {
    const keys = storageKeys();
    window.localStorage.setItem(keys.workspace, selection.workspaceId);
    if (selection.tabId) window.localStorage.setItem(keys.tab, selection.tabId);
    else window.localStorage.removeItem(keys.tab);
  } catch {}
}
