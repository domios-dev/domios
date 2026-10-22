// Registry of tab-scoped clipboard callbacks.
// TerminalTab registers on mount; the app shortcut dispatcher calls it for the focused tab.

export interface TabClipboard {
  copy: () => void;
  paste: () => void;
}

const registry = new Map<string, TabClipboard>();

export function registerTabClipboard(tabId: string, clipboard: TabClipboard): () => void {
  registry.set(tabId, clipboard);
  return () => {
    if (registry.get(tabId) === clipboard) {
      registry.delete(tabId);
    }
  };
}

/** False when no tab with that id is registered (fresh startup, or a tab that just unmounted). */
export function pasteToTab(tabId: string): boolean {
  const clipboard = registry.get(tabId);
  if (!clipboard) return false;
  clipboard.paste();
  return true;
}

export function copyFromTab(tabId: string): boolean {
  const clipboard = registry.get(tabId);
  if (!clipboard) return false;
  clipboard.copy();
  return true;
}
