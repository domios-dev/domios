export type RemoteTransportAvailability = "reconnecting" | "unavailable" | "available";
export type RemoteTransportChannel = "control" | "output" | "chat";

export type RemoteChannelAvailabilityMap = Partial<Record<RemoteTransportChannel, RemoteTransportAvailability>>;

// A channel is usable while transient ('available'/'reconnecting') or unknown; only an
// explicit 'unavailable' takes it out. Unknown stays optimistic to match fresh projection.
export function channelUsable(availability: RemoteTransportAvailability | undefined): boolean {
  return availability !== "unavailable";
}

// D2: terminal (output+control) usability is independent of the chat channel.
export function remoteTerminalUsable(channels: RemoteChannelAvailabilityMap | undefined): boolean {
  return channelUsable(channels?.control) && channelUsable(channels?.output);
}

export function remoteChatUsable(channels: RemoteChannelAvailabilityMap | undefined): boolean {
  return channelUsable(channels?.chat);
}

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
