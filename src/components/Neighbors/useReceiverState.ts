import { receiverState, type NeighborRuntimeClient, type ReceiverState } from "./neighborRuntimeClient";
import { useNeighborStatus } from "./neighborStatusStore";

/** Which machine receives Neighbor messages, from the shared status snapshot; unknown until the daemon's watcher reports. */
export function useReceiverState(client: NeighborRuntimeClient, active: boolean): ReceiverState | null {
  const status = useNeighborStatus(client, active);
  if (!status || status.receiver_status_known === false || !receiverState(status)) return null;
  return status;
}

export function receivingLine(state: ReceiverState): string {
  if (state.receiving_here) return "Receiving here";
  if (state.receiving_enabled) return `Receiving moved to ${state.receiver_machine_label ?? "another machine"}`;
  return state.revision === 0 ? "Receiver not chosen yet" : "Not receiving";
}
