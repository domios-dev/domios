/** Event the tray dispatches and ExtensionsView listens for, to deep-link into a
 * plugin's detail (id === null clears the selection / opens the list root). */
export const SELECT_EXTENSION_EVENT = "codeterm:select-extension";

let pendingSelection: { id: string | null } | null = null;

/** Open the Extensions surface, deep-linking to `id` (or the root when null). */
export function selectExtension(id: string | null, onOpen: () => void): void {
  pendingSelection = { id };
  window.dispatchEvent(new CustomEvent(SELECT_EXTENSION_EVENT, { detail: { id } }));
  onOpen();
}

/** The selection requested before Extensions first mounted, consumed once so a live listener and a cold mount agree. */
export function takePendingExtensionSelection(): string | null | undefined {
  const selection = pendingSelection;
  pendingSelection = null;
  return selection?.id;
}
