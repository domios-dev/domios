// Registry of "paste into THIS pane" callbacks, keyed by pane id. Each
// TerminalPane registers on mount, App.tsx looks up and calls directly when
// the app-wide Ctrl+V / Cmd+V shortcut fires — synchronous, no event-bus
// indirection that can swallow the event in remount races.

type PasteFn = () => void;

const registry = new Map<string, PasteFn>();

export function registerPanePaste(paneId: string, paste: PasteFn): () => void {
  registry.set(paneId, paste);
  return () => {
    if (registry.get(paneId) === paste) {
      registry.delete(paneId);
    }
  };
}

/** Returns true when a pane handled the paste, false when no matching
 *  pane is registered (typical for fresh startup or a pane that was just
 *  unmounted). The caller decides whether to fall through to other paths. */
export function pasteToPane(paneId: string): boolean {
  const paste = registry.get(paneId);
  if (!paste) return false;
  paste();
  return true;
}
