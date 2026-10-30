import { receiverState, type NeighborRuntimeClient, type ReceiverState } from "./neighborRuntimeClient";
import { useNeighborStatus } from "./neighborStatusStore";
import type { NeighborStatusSnapshot } from "./neighborStatusStore";

export function friendReceiverState(status: NeighborStatusSnapshot | null, friendship: string): ReceiverState | null {
  const receiver = status?.friend_receivers?.[friendship];
  return receiverState(receiver) ? receiver : null;
}

export function useReceiverState(client: NeighborRuntimeClient, active: boolean, friendship: string): ReceiverState | null {
  return friendReceiverState(useNeighborStatus(client, active), friendship);
}

export function receivingLine(state: ReceiverState): string {
  if (state.receiving_here) return `Receiving on ${state.receiver_machine_label ?? "this machine"}`;
  if (state.receiving_enabled) return `Receiving on ${state.receiver_machine_label ?? "another machine"}`;
  return state.revision === 0 ? "Receiver not chosen yet" : "Not receiving";
}
