import { receiverLocation } from "./neighborReceiverLocationModel";
import { localJson, relayPeerUsable, selfHost } from "../../lib/webApi";
import { webLinkWithTokenAndRelay } from "../../lib/webAccess";
import { nativeBridge } from "../../lib/nativeBridge";
import { publishNotice } from "../../lib/notices";
import type { MeshPeersResponse, MeshStatus } from "../../lib/types";
import type { DropdownMenuItem } from "../shared/DropdownMenu";

export function receiverOpenItem(receiver: unknown, label: string): DropdownMenuItem | null {
  const location = receiverLocation(receiver);
  if (!location.installationId) return null;
  const expired = typeof receiver === "object" && receiver !== null
    && "not_receiving_reason" in receiver && receiver.not_receiving_reason === "membership_expired";
  const available = location.instanceId !== null && location.online === true && !expired;
  return {
    label: `Open on ${label}`,
    unavailable: !available,
    description: expired ? `${label}'s membership expired` : available ? undefined : location.online === false ? "That computer is offline." : "Machine connection unavailable",
    onClick: () => {
      if (!location.installationId || !location.instanceId || !available) return;
      void openReceiver(location.installationId, location.instanceId).catch((failure: unknown) => publishNotice({
        kind: "toast", severity: "Error", id: `neighbor-location:${location.installationId}`,
        title: "Could not open concierge machine", detail: failure instanceof Error ? failure.message : "Try again when the machine is online.",
      }));
    },
  };
}

export async function openReceiver(installationId: string, instanceId: string): Promise<void> {
  const [peers, status] = await Promise.all([
    localJson<MeshPeersResponse>("GET", "/api/mesh/peers"),
    localJson<MeshStatus>("GET", "/api/mesh/status"),
  ]);
  const installation = (peers.installations ?? []).find(row => row.installation_id === installationId);
  if (!installation?.active || installation.signed_out) throw new Error("The concierge machine is unavailable. Refresh its receiving status.");
  const self = instanceId === status.instance_id;
  if (!self && (installation.instance_id !== instanceId || !relayPeerUsable(peers.paired.find(peer => peer.instance_id === instanceId)))) {
    throw new Error("The concierge machine is offline or its connection changed.");
  }
  await nativeBridge.openUrl(webLinkWithTokenAndRelay(selfHost(), null, self ? null : { id: instanceId }));
}
