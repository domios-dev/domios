import { receiverState, type NeighborRuntimeClient, type ReceiverState } from "./neighborRuntimeClient";
import { useNeighborStatus } from "./neighborStatusStore";
import type { NeighborStatusSnapshot } from "./neighborStatusStore";

export function friendReceiverState(status: NeighborStatusSnapshot | null, friendship: string): ReceiverState | null {
  const receiver = status?.friend_receivers?.[friendship];
  return receiverState(receiver) ? receiver : null;
}

/** Which machine receives Neighbor messages, from the shared status snapshot; unknown until the daemon's watcher reports. */
export function useReceiverState(client: NeighborRuntimeClient, active: boolean, friendship?: string): ReceiverState | null {
  const status = useNeighborStatus(client, active);
  if (friendship !== undefined) return friendReceiverState(status, friendship);
  if (!status || status.receiver_status_known === false || !receiverState(status)) return null;
  return status;
}

export function receivingLine(state: ReceiverState): string {
  if (state.receiving_here) return "Receiving here";
  if (state.receiving_enabled) return `Receiving on ${state.receiver_machine_label ?? "another machine"}`;
  return state.revision === 0 ? "Receiver not chosen yet" : "Not receiving";
}
