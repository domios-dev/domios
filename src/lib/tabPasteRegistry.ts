// Registry of tab-scoped paste callbacks.
// TerminalTab registers on mount; App.tsx calls it directly on global paste shortcuts.

type PasteFn = () => void;

const registry = new Map<string, PasteFn>();

export function registerTabPaste(tabId: string, paste: PasteFn): () => void {
  registry.set(tabId, paste);
  return () => {
    if (registry.get(tabId) === paste) {
      registry.delete(tabId);
    }
  };
}

/** Returns true when a tab handled the paste, false when no matching
 *  tab is registered (typical for fresh startup or a tab that was just
 *  unmounted). The caller decides whether to fall through to other paths. */
export function pasteToTab(tabId: string): boolean {
  const paste = registry.get(tabId);
  if (!paste) return false;
  paste();
  return true;
}
