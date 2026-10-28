import { useState } from "react";
import type { DropdownMenuItem } from "../shared/DropdownMenu";
import { NeighborReceiveHere } from "./NeighborReceiveHere";
import type { NeighborRuntimeClient, ReceiverState } from "./neighborRuntimeClient";

/** Receiving one friend here ("set"), or handing that friend back to the account's receiver ("reset"). */
export type FriendReceiveOperation = "set" | "reset";

/** Where one friend's messages arrive, in the few words a row or a header has room for. */
export function friendReceivingLine(receiver: ReceiverState): string {
  if (receiver.receiving_here) return "receiving here";
  if (receiver.receiving_enabled) return `receiving on ${receiver.receiver_machine_label ?? "another machine"}`;
  return "not receiving";
}

/** The receiving actions a friend's menus offer: receive here when this machine does not, and undo an override. */
export function friendReceiveItems(receiver: ReceiverState | null, open: (operation: FriendReceiveOperation) => void): DropdownMenuItem[] {
  if (!receiver) return [];
  return [
    ...(!receiver.receiving_here ? [{ label: "Receive this friend here", onClick: () => open("set") }] : []),
    ...(receiver.source === "friend" ? [{ label: "Use account receiver", onClick: () => open("reset") }] : []),
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
  return <NeighborReceiveHere key={`${request.friendship}:${request.operation}`} receiver={receiver} friendName={friendName} targetMachine={targetMachine}
    operation={request.operation} initiallyOpen onClose={onClose}
    onReceive={revision => client.receiveFriend(request.friendship, revision, request.operation)} />;
}
