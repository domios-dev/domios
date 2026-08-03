export type RemoteTransportAvailability = "reconnecting" | "unavailable" | "available";
export type RemoteTransportChannel = "control" | "output" | "chat";

export interface RemoteTransportStateChange {
  ordinaryTabId: string;
  channel: RemoteTransportChannel;
  availability: RemoteTransportAvailability;
}

const EVENT = "codeterm-remote-transport-state";

export function publishRemoteTransportState(
  ordinaryTabId: string,
  channel: RemoteTransportChannel,
  availability: RemoteTransportAvailability,
): void {
  window.dispatchEvent(new CustomEvent(EVENT, { detail: { ordinaryTabId, channel, availability } }));
}

export function onRemoteTransportState(
  handler: (detail: RemoteTransportStateChange) => void,
): () => void {
  const listener = (event: Event) => handler((event as CustomEvent).detail);
  window.addEventListener(EVENT, listener);
  return () => window.removeEventListener(EVENT, listener);
}
