const EVENT = "codeterm:neighbor-focus";
let pending: string | null = null;

/** Opens the neighborhood with one connection selected; the page may mount after this call. */
export function openNeighbor(friendshipId: string) {
  pending = friendshipId;
  window.dispatchEvent(new CustomEvent(EVENT));
  window.dispatchEvent(new CustomEvent("codeterm:open-panel", { detail: { panel: "neighbors" } }));
}

export function takeNeighborFocus(): string | null {
  const value = pending;
  pending = null;
  return value;
}

export function onNeighborFocus(handler: () => void): () => void {
  window.addEventListener(EVENT, handler);
  return () => window.removeEventListener(EVENT, handler);
}
