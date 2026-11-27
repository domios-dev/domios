import { cloudMachineCardView, type CloudListIcon } from "../../lib/cloudMachines/stateModel";
import type { CloudReceipt, EnrichedPairedPeer } from "../../lib/cloudMachines/types";
import { cleanMachineName } from "../../lib/machineIdentity";
import { RELAY_PROTOCOL_VERSION, type MeshPeersResponse } from "../../lib/types";

const offlineClock = new Intl.DateTimeFormat(undefined, { hour: "2-digit", minute: "2-digit" });

export function resolvedPeerName(
  peer: Pick<EnrichedPairedPeer, "instance_id" | "instance_name" | "kind" | "cloud">,
  accountPeerName?: string,
): string {
  const cloudName = peer.kind === "cloud" ? cleanMachineName(peer.cloud?.name, peer.instance_id) : undefined;
  return cloudName
    ?? accountPeerName
    ?? cleanMachineName(peer.instance_name, peer.instance_id)
    ?? "Machine";
}

export function unpairedCloudMachines(
  peers: readonly EnrichedPairedPeer[],
  cloudMachines: readonly CloudReceipt[],
  installations?: MeshPeersResponse["installations"],
): CloudReceipt[] {
  const pairedIds = new Set(peers.map((peer) => peer.instance_id));
  const pairedCloudIds = new Set(
    peers.flatMap((peer) => (peer.cloud?.machine_id ? [peer.cloud.machine_id] : [])),
  );
  const pairedInstallationIds = new Set(
    (installations ?? []).flatMap((row) =>
      row.active && row.instance_id && pairedIds.has(row.instance_id) ? [row.installation_id] : [],
    ),
  );
  return cloudMachines.filter((machine) => {
    if (pairedCloudIds.has(machine.machine_id)) return false;
    if (machine.peer_instance_id && pairedIds.has(machine.peer_instance_id)) return false;
    if (machine.installation_id && (pairedInstallationIds.has(machine.installation_id) || pairedIds.has(machine.installation_id))) {
      return false;
    }
    return true;
  });
}

export function peerStatus(
  peer: EnrichedPairedPeer,
  availability?: string,
): { text: string; icon: CloudListIcon; online: boolean; cloud: boolean } {
  if (peer.kind === "cloud" && peer.cloud) {
    const view = cloudMachineCardView(peer.cloud);
    if (!view.listOnline) {
      return { text: view.listStatus, icon: view.listIcon, online: false, cloud: true };
    }
    if (availability === "reconnecting") {
      return { text: "reconnecting", icon: "dots", online: false, cloud: true };
    }
    if (availability === "unreachable") {
      return { text: "unreachable", icon: "dot", online: false, cloud: true };
    }
    if (availability === "signed-out") {
      return { text: "signed out", icon: "dot", online: false, cloud: true };
    }
    if (availability === "update-required" || (peer.online && peer.relay_protocol_version !== RELAY_PROTOCOL_VERSION)) {
      return { text: "update required", icon: "dot", online: true, cloud: true };
    }
    if (availability === "offline" || !peer.online) {
      return { text: "offline", icon: "dot", online: false, cloud: true };
    }
    return { text: view.listStatus, icon: view.listIcon, online: true, cloud: true };
  }
  if (availability === "reconnecting") {
    return { text: "reconnecting", icon: "dots", online: false, cloud: false };
  }
  if (availability === "unreachable") {
    return { text: "unreachable", icon: "dot", online: false, cloud: false };
  }
  if (peer.online) {
    return {
      text: peer.relay_protocol_version === RELAY_PROTOCOL_VERSION ? "online" : "update required",
      icon: "dot",
      online: true,
      cloud: false,
    };
  }
  const seen = peer.last_seen ? Date.parse(peer.last_seen) : NaN;
  return {
    text: Number.isFinite(seen) ? `offline since ${offlineClock.format(seen)}` : "offline",
    icon: "dot",
    online: false,
    cloud: false,
  };
}
