import { useState } from "react";
import type { DropdownMenuItem } from "../shared/DropdownMenu";
import { NeighborReceiveHere } from "./NeighborReceiveHere";
import type { NeighborRuntimeClient, ReceiverState } from "./neighborRuntimeClient";
import { notReceivingReason } from "./useReceiverState";
import { receiverOpenItem } from "./neighborReceiverLocation";
import { receiverLocation } from "./neighborReceiverLocationModel";

export type FriendReceiveOperation = "set" | "reset";

/** Where one friend's messages arrive, in the few words a row or a header has room for. */
export function friendReceivingLine(receiver: ReceiverState | null): string {
  if (!receiver) return "concierge location unknown";
  if (receiver.receiving_here) return "receiving here";
  const reason = notReceivingReason(receiver);
  if (reason) return `not receiving · ${reason}`;
  if (receiver.receiving_enabled) return `${receiverLocation(receiver).online === false ? "offline" : "receiving"} on ${receiver.receiver_machine_label ?? "another machine"}`;
  if (receiver.receiver_machine_label) return `not receiving · concierge on ${receiver.receiver_machine_label}`;
  if (receiver.revision === 0) return "concierge machine not chosen";
  return "not receiving";
}


export function friendReceiveItems(receiver: ReceiverState | null, open: (operation: FriendReceiveOperation) => void): DropdownMenuItem[] {
  if (!receiver) return [];
  const openItem = !receiver.receiving_here ? receiverOpenItem(receiver, receiver.receiver_machine_label ?? "concierge machine") : null;
  return [
    ...(openItem ? [openItem] : []),
    ...(!receiver.receiving_here ? [{ label: "Receive this friend here", onClick: () => open("set") }] : []),
    ...(receiver.receiving_here ? [{ label: "Stop receiving this friend", onClick: () => open("reset") }] : []),
  ];
}

/** The open confirmation of one friend's receiving, wherever its action was chosen. */
export interface FriendReceiveRequest { friendship: string; operation: FriendReceiveOperation }
export function useFriendReceiveRequest() {
  const [request, setRequest] = useState<FriendReceiveRequest | null>(null);
  return { request, open: (friendship: string, operation: FriendReceiveOperation) => setRequest({ friendship, operation }), close: () => setRequest(null) };
}

/** One friend's confirmation, through the shared receive component; only this friend moves. */
export function FriendReceiveConfirm({ request, receiver, friendName, targetMachine, client, onClose }: {
  request: FriendReceiveRequest; receiver: ReceiverState; friendName: string; targetMachine?: string; client: NeighborRuntimeClient; onClose: () => void;
}) {
  return <NeighborReceiveHere key={`${request.friendship}:${request.operation}`} friendship={request.friendship} client={client} receiver={receiver} friendName={friendName} targetMachine={targetMachine}
    operation={request.operation} initiallyOpen onClose={onClose}
    onReceive={(revision, decision) => decision ? client.receiveFriend(request.friendship, revision, request.operation, decision) : client.receiveFriend(request.friendship, revision, request.operation)} />;
}
