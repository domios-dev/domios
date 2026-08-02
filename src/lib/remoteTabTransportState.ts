export type RemoteTransportAvailability = "reconnecting" | "unavailable" | "available";

const EVENT = "codeterm-remote-transport-state";

export function publishRemoteTransportState(ordinaryTabId: string, availability: RemoteTransportAvailability): void {
  window.dispatchEvent(new CustomEvent(EVENT, { detail: { ordinaryTabId, availability } }));
}

export function onRemoteTransportState(
  handler: (detail: { ordinaryTabId: string; availability: RemoteTransportAvailability }) => void,
): () => void {
  const listener = (event: Event) => handler((event as CustomEvent).detail);
  window.addEventListener(EVENT, listener);
  return () => window.removeEventListener(EVENT, listener);
}
