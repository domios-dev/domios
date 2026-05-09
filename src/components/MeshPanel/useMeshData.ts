import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { MeshStatus, MeshPeersResponse, PairedPeer } from "../../lib/types";
import { servicesList } from "./meshApi";
import type { Service } from "./types";

export interface PeerStatus {
  instance_id: string;
  name: string;
  ip: string;
  api_port: number;
  online: boolean;
  access_level: string;
}

export interface MeshData {
  thisMachine: { name: string; ip: string };
  peers: PeerStatus[];
  services: Service[];
  loading: boolean;
  error: string | null;
}

const DEFAULT_DATA: MeshData = {
  thisMachine: { name: "this", ip: "127.0.0.1" },
  peers: [],
  services: [],
  loading: true,
  error: null,
};

export interface MeshDataResult extends MeshData {
  refresh: () => void;
}

export function useMeshData(refreshKey: number): MeshDataResult {
  const [data, setData] = useState<MeshData>(DEFAULT_DATA);

  const loadAll = useCallback(async () => {
    setData((d) => ({ ...d, loading: true }));
    try {
      const local: Service[] = await servicesList();
      const status = await invoke<MeshStatus>("mesh_get_status").catch(
        () => null,
      );
      const meshPeers = await invoke<MeshPeersResponse>("mesh_list_peers").catch(
        () => ({ paired: [], discovered: [], incoming_requests: [] }) as MeshPeersResponse,
      );
      const paired: PeerStatus[] = (meshPeers.paired ?? []).map(
        (p: PairedPeer) => ({
          instance_id: p.instance_id,
          name: p.instance_name,
          ip: p.addresses[0]?.ip ?? "",
          api_port: p.api_port,
          online: p.online,
          access_level: p.access_level,
        }),
      );

      const remotePromises = paired
        .filter((p) => p.online && p.ip)
        .map(async (p) => {
          try {
            const arr = await invoke<Service[]>("mesh_peer_services_list", {
              instanceId: p.instance_id,
            });
            return arr.map((s) => ({ ...s, source_peer: p.name }));
          } catch {
            return [];
          }
        });
      const remoteResults = await Promise.all(remotePromises);
      const remote = remoteResults.flat();

      setData({
        thisMachine: {
          name: status?.instance_name ?? "this",
          ip: status?.local_ip ?? status?.tailscale_ip ?? "",
        },
        peers: paired,
        services: [...local, ...remote],
        loading: false,
        error: null,
      });
    } catch (e) {
      setData((d) => ({ ...d, loading: false, error: String(e) }));
    }
  }, []);

  useEffect(() => {
    loadAll();
    const handle = setInterval(() => { loadAll(); }, 5000);
    return () => clearInterval(handle);
  }, [loadAll, refreshKey]);

  return { ...data, refresh: loadAll };
}
