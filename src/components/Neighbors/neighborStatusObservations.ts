import type { ChannelStatus, ReceiverState } from "./neighborRuntimeClient";

export interface NeighborStatusObservation {
  receiver: ReceiverState | null;
  concierge: unknown;
  stale: boolean;
}

export type ObservedNeighborStatus = Omit<ChannelStatus, "concierges"> & {
  concierges?: Record<string, unknown>;
  observations?: Record<string, NeighborStatusObservation>;
  status_stale?: boolean;
};

type StatusEvidence = Pick<ObservedNeighborStatus, "bindings" | "friend_receivers" | "concierges" | "observations" | "status_stale">;

export function observeNeighborStatus<T extends StatusEvidence>(fresh: T, current?: Pick<StatusEvidence, "bindings" | "observations">): T & { observations: Record<string, NeighborStatusObservation> } {
  const observations: Record<string, NeighborStatusObservation> = {};
  const ids = new Set([...Object.keys(fresh.bindings), ...Object.keys(fresh.friend_receivers ?? {}), ...Object.keys(current?.observations ?? {})]);
  for (const id of ids) {
    const receiver = fresh.friend_receivers?.[id];
    const previous = current?.observations?.[id];
    const concierge = fresh.concierges?.[id];
    const localLifecycle = typeof concierge === "object" && concierge !== null
      && "gateway_tab" in concierge && concierge.gateway_tab === fresh.bindings[id]
      && "state" in concierge && concierge.state !== "not_receiving" && concierge.state !== "receiving_elsewhere";
    observations[id] = {
      receiver: receiver ?? previous?.receiver ?? null,
      concierge: receiver || localLifecycle ? concierge : fresh.bindings[id] === current?.bindings[id] ? previous?.concierge : undefined,
      stale: fresh.status_stale === true || !receiver,
    };
  }
  return { ...fresh, observations };
}
