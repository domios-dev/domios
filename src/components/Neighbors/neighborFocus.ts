const EVENT = "codeterm:neighbor-focus";

export interface NeighborFocus { id: string; section: "configure" | null }
let pending: NeighborFocus | null = null;

/** Opens one Neighbor's page in the neighborhood; the page may mount after this call. */
export function openNeighborPage(friendshipId: string, section: NeighborFocus["section"] = null) {
  pending = { id: friendshipId, section };
  window.dispatchEvent(new CustomEvent(EVENT));
  window.dispatchEvent(new CustomEvent("codeterm:open-panel", { detail: { panel: "neighbors" } }));
}

export function takeNeighborFocus(): NeighborFocus | null {
  const value = pending;
  pending = null;
  return value;
}

export function onNeighborFocus(handler: () => void): () => void {
  window.addEventListener(EVENT, handler);
  return () => window.removeEventListener(EVENT, handler);
}

/** Opens the conversation with one Neighbor; notification actions (domios://neighbor/<id>) land here too. */
export function openNeighbor(friendshipId: string) {
  window.dispatchEvent(new CustomEvent("codeterm:open-panel", { detail: { panel: `neighbor:${friendshipId}` } }));
}
