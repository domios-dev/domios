import { apiHost, relayTarget } from "./webApi";
import type { SavedPageSelection } from "../store/multiplexer/pageSelection";

function storageKey(): string {
  let origin = window.location?.origin ?? "local";
  try { origin = apiHost(); } catch {}
  return `codeterm:page-selection:${origin}:${relayTarget()?.id ?? "self"}`;
}

export function loadClientPageSelection(): SavedPageSelection | null {
  try {
    const value = JSON.parse(window.localStorage.getItem(storageKey()) ?? "null") as unknown;
    if (value && typeof value === "object" && "workspaceId" in value && typeof value.workspaceId === "string"
      && "tabId" in value && (value.tabId === null || typeof value.tabId === "string")) return value as SavedPageSelection;
  } catch {}
  return null;
}

export function saveClientPageSelection(selection: SavedPageSelection): void {
  try { window.localStorage.setItem(storageKey(), JSON.stringify(selection)); } catch {}
}
