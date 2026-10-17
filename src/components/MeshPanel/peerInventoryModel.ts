import type { Service } from "./types";
import type { PeerStatus } from "./useMeshData";
import { buildInventoryRows, inventoryFreshness, sortInventoryRows } from "./meshInventoryModel";

export function peerInventoryModel(resources: Service[], peer: Pick<PeerStatus, "instance_id" | "name" | "online" | "pending" | "services_loaded" | "services_stale" | "services_error">, isSelf = false, loading = false, error?: string | null) {
  const selected = resources.filter((resource) => {
    if (isSelf) return !resource.source_peer_id && !resource.source_peer;
    const id = resource.source_peer_id ?? resource.machine_id;
    return id ? id === peer.instance_id : resource.source_peer === peer.name;
  });
  const rows = sortInventoryRows(buildInventoryRows(selected, peer.name), { key: "name", direction: "asc" });
  const unavailable = error || peer.pending || !peer.online || peer.services_error || peer.services_stale
    || (!isSelf && !loading && !peer.services_loaded) || selected.some((resource) => inventoryFreshness(resource) !== "fresh" || resource.provenance?.partial_error);
  const state = unavailable ? "unavailable" : loading ? "loading" : "ready";
  return {
    services: rows.filter((row) => row.kind === "daemon"),
    crons: rows.filter((row) => row.kind === "cron"),
    state,
    message: state === "unavailable"
      ? peer.pending ? "Inventory unavailable until paired" : !peer.online ? "Machine offline · inventory unavailable" : "Inventory unavailable"
      : state === "loading" ? "Refreshing inventory" : null,
  };
}

export type PeerInventory = ReturnType<typeof peerInventoryModel>;
