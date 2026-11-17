import { receiverState, type NeighborRuntimeClient, type ReceiverState } from "./neighborRuntimeClient";
import { useNeighborStatus } from "./neighborStatusStore";
import type { NeighborStatusSnapshot } from "./neighborStatusStore";

export function friendReceiverState(status: NeighborStatusSnapshot | null, friendship: string, listed?: unknown): ReceiverState | null {
  const receiver = status?.friend_receivers?.[friendship];
  const lastKnown = status?.observations?.[friendship]?.receiver;
  if (typeof listed === "object" && listed !== null && "receiving" in listed && listed.receiving === false
    && "installation_id" in listed && listed.installation_id === null && "enc_key_id" in listed && listed.enc_key_id === null
    && "revision" in listed && typeof listed.revision === "number" && Number.isSafeInteger(listed.revision) && listed.revision >= 0
    && listed.revision > (receiver?.revision ?? -1) && listed.revision >= (lastKnown?.revision ?? -1)) {
    return { receiving_here: false, is_me: false, receiving_enabled: false, receiver_machine_label: null, revision: listed.revision };
  }
  return receiverState(receiver) ? receiver : null;
}

export function useReceiverState(client: NeighborRuntimeClient, active: boolean, friendship: string, listed?: unknown): ReceiverState | null {
  const status = useNeighborStatus(client, active);
  return active ? friendReceiverState(status, friendship, listed) : null;
}

/** Why the chosen machine receives nothing now, naming it only as the reason. */
export function notReceivingReason(state: ReceiverState): string | null {
  return state.not_receiving_reason === "membership_expired" ? `${state.receiver_machine_label ?? "its machine"}'s membership expired` : null;
}

export function receivingLine(state: ReceiverState): string {
  const reason = notReceivingReason(state);
  if (reason) return `Not receiving — ${reason}`;
  if (state.receiving_here) return `Receiving on ${state.receiver_machine_label ?? "this machine"}`;
  if (state.receiving_enabled) return `Receiving on ${state.receiver_machine_label ?? "another machine"}`;
  return state.revision === 0 ? "Receiver not chosen yet" : "Not receiving";
}
