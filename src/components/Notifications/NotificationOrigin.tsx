import type { Notification } from "../../lib/types";
import { MachineProvenance } from "../shared/MachineProvenance";

export function NotificationOrigin({ notification }: { notification: Pick<Notification, "peer_name" | "remote_origin"> }) {
  if (notification.remote_origin) {
    return <MachineProvenance peerId={notification.remote_origin.peerId} name={notification.remote_origin.peerName} remote />;
  }
  return notification.peer_name ? <MachineProvenance name={notification.peer_name} remote /> : null;
}
