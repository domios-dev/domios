/** Event the tray dispatches and ExtensionsView listens for, to deep-link into a
 * plugin's detail (id === null clears the selection / opens the list root). */
export const SELECT_EXTENSION_EVENT = "codeterm:select-extension";

/** Open the Extensions surface, deep-linking to `id` (or the root when null). */
export function selectExtension(id: string | null, onOpen: () => void): void {
  window.dispatchEvent(new CustomEvent(SELECT_EXTENSION_EVENT, { detail: { id } }));
  onOpen();
}
